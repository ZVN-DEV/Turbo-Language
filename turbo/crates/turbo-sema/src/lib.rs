use std::collections::HashMap;
use turbo_ast::*;

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
    /// Array of a given element type
    Array(Box<Ty>),
    /// Struct type (by name)
    Struct(String),
    /// Enum type (by name)
    Enum(String),
    /// Function type: fn(params) -> ret
    Fn(Vec<Ty>, Box<Ty>),
    /// Result type: Result<ok, err>
    Result(Box<Ty>, Box<Ty>),
    /// Optional type: Optional<inner>
    Optional(Box<Ty>),
    /// Future<T> — result of spawn / async function call
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
            Ty::I32 => write!(f, "i32"),
            Ty::I64 => write!(f, "i64"),
            Ty::U32 => write!(f, "u32"),
            Ty::U64 => write!(f, "u64"),
            Ty::F32 => write!(f, "f32"),
            Ty::F64 => write!(f, "f64"),
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

    /// Check if this type contains `Ty::Error` anywhere in its structure
    /// (e.g., `Optional(Error)`, `Result(Error, Error)`, `Array(Error)`).
    /// Used to suppress cascading diagnostics when the inner error was already reported.
    fn contains_error(&self) -> bool {
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

/// Check if two types are compatible (allowing partial Result types where Error = unknown).
fn types_compatible(expected: &Ty, actual: &Ty) -> bool {
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

fn resolve_type_expr(
    te: &TypeExpr,
    structs: Option<&HashMap<String, StructInfo>>,
    enums: Option<&HashMap<String, EnumInfo>>,
) -> Option<Ty> {
    resolve_type_expr_with_params(te, structs, enums, &[])
}

fn resolve_type_expr_with_params(
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
                "i32" => Some(Ty::I32),
                "i64" => Some(Ty::I64),
                "u32" => Some(Ty::U32),
                "u64" => Some(Ty::U64),
                "f32" => Some(Ty::F32),
                "f64" => Some(Ty::F64),
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

/// Variable info in scope
#[derive(Debug, Clone)]
struct VarInfo {
    ty: Ty,
    mutable: bool,
}

/// Function signature
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct FnSig {
    type_params: Vec<String>,
    type_param_bounds: HashMap<String, Vec<String>>,
    params: Vec<(String, Ty)>,
    ret: Ty,
    is_async: bool,
    is_tool: bool,
    is_unsafe: bool,
}

/// Scope for variable tracking
struct Scope {
    vars: HashMap<String, VarInfo>,
}

/// Registered agent info for semantic checking
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct AgentInfo {
    model: String,
    tools: Vec<String>,
    system_prompt: Option<String>,
}

/// Struct field info for the checker
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct StructInfo {
    fields: Vec<(String, Ty)>,
    /// Type parameter names for generic structs
    type_params: Vec<String>,
    /// Derived trait names from `@derive(...)` attribute
    derives: Vec<String>,
}

/// Enum info (variant names + field types)
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct EnumInfo {
    /// Variant name -> field types (empty vec for unit variants)
    variants: Vec<(String, Vec<Ty>)>,
    /// Type parameter names for generic enums
    type_params: Vec<String>,
}

impl EnumInfo {
    /// Get just the variant names
    fn variant_names(&self) -> Vec<String> {
        self.variants.iter().map(|(name, _)| name.clone()).collect()
    }

    /// Check if a variant name exists
    fn has_variant(&self, name: &str) -> bool {
        self.variants.iter().any(|(n, _)| n == name)
    }

    /// Get the field types for a variant
    fn variant_fields(&self, name: &str) -> Option<&Vec<Ty>> {
        self.variants
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, fields)| fields)
    }
}

/// Trait definition info for the checker
#[derive(Debug, Clone)]
struct TraitInfo {
    methods: Vec<TraitMethodInfo>,
}

/// Trait method signature info
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct TraitMethodInfo {
    name: String,
    params: Vec<(String, Ty)>,
    ret: Ty,
    /// Whether this method has a default implementation in the trait
    has_default: bool,
}

/// Type checker
#[allow(dead_code)]
struct Checker {
    errors: Vec<SemaError>,
    functions: HashMap<String, FnSig>,
    agents: HashMap<String, AgentInfo>,
    structs: HashMap<String, StructInfo>,
    enums: HashMap<String, EnumInfo>,
    /// Methods: type_name -> method_name -> FnSig
    methods: HashMap<String, HashMap<String, FnSig>>,
    /// Trait definitions: trait_name -> TraitInfo
    traits: HashMap<String, TraitInfo>,
    /// Trait implementations: type_name -> set of trait names
    trait_impls: HashMap<String, Vec<String>>,
    /// Module-level constants: name -> type
    constants: HashMap<String, Ty>,
    scopes: Vec<Scope>,
    /// Return type of the current function being checked
    current_return_type: Ty,
    /// Hint for closure parameter types when checking closures passed to map/filter/reduce
    closure_param_hint: Option<Vec<Ty>>,
    /// When true, `main` is not required (used for `turbolang test` mode)
    test_mode: bool,
    /// Whether we are currently checking inside an `@unsafe` function
    in_unsafe_context: bool,
    /// Nesting depth of loops (for break/continue validation)
    loop_depth: usize,
    /// Current recursion depth for expression checking (prevents stack overflow)
    expr_depth: usize,
}

/// Maximum recursion depth for expression/statement checking.
/// Exceeding this produces a sema error instead of a stack overflow.
const MAX_EXPR_DEPTH: usize = 200;

impl Checker {
    fn new() -> Self {
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

    fn error(&mut self, code: ErrorCode, message: String, span: Span) {
        self.errors.push(SemaError {
            code,
            message,
            span,
        });
    }

    // === Scope management ===

    fn push_scope(&mut self) {
        self.scopes.push(Scope {
            vars: HashMap::new(),
        });
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

    fn is_builtin_function(name: &str) -> bool {
        matches!(
            name,
            "print"
                | "panic"
                | "assert"
                | "assert_eq"
                | "assert_ne"
                | "len"
                | "abs"
                | "min"
                | "max"
                | "to_str"
                | "map"
                | "filter"
                | "reduce"
                | "split"
                | "trim"
                | "upper"
                | "lower"
                | "starts_with"
                | "ends_with"
                | "replace"
                | "char_at"
                | "contains"
                | "index_of"
                | "join"
                | "repeat"
                | "read_line"
                | "read_file"
                | "write_file"
                | "pow"
                | "sqrt"
                | "sleep"
                | "http_get"
                | "http_post"
                | "json_get"
                | "json_stringify"
                | "http_server"
                | "route"
                | "http_listen"
                | "respond"
                | "request_body"
                | "channel"
                | "send"
                | "recv"
                | "mutex"
                | "mutex_get"
                | "mutex_set"
                | "clone"
                | "hashmap"
                | "hashmap_set"
                | "hashmap_get"
                | "hashmap_has"
                | "hashmap_len"
                | "hashmap_keys"
                | "hashmap_remove"
                | "to_json"
                | "to_json_array"
                | "deref"
                | "store"
        )
    }

    /// Walk a chain of FieldAccess / Index expressions to find the root variable name.
    fn root_var_name(expr: &Spanned<Expr>) -> Option<String> {
        match &expr.node {
            Expr::Ident(name) => Some(name.clone()),
            Expr::FieldAccess { object, .. } => Self::root_var_name(object),
            Expr::Index { object, .. } => Self::root_var_name(object),
            _ => None,
        }
    }

    fn check_module(&mut self, module: &Module) {
        // Pass 0: register all struct definitions
        for item in &module.items {
            let Item::Struct(s) = &item.node else {
                continue;
            };
            if self.structs.contains_key(&s.name) {
                self.error(
                    ErrorCode::E0306,
                    format!("struct `{}` is already defined", s.name),
                    item.span.clone(),
                );
                continue;
            }
            let tp_names: Vec<String> = s.type_param_names();
            let mut fields = Vec::new();
            for field in &s.fields {
                match resolve_type_expr_with_params(
                    &field.ty.node,
                    Some(&self.structs),
                    Some(&self.enums),
                    &tp_names,
                ) {
                    Some(ty) => fields.push((field.name.clone(), ty)),
                    None => {
                        if let TypeExpr::Named(name) = &field.ty.node {
                            self.error(
                                ErrorCode::E0305,
                                format!("unknown type `{name}` in struct `{}`", s.name),
                                field.ty.span.clone(),
                            );
                        }
                        fields.push((field.name.clone(), Ty::Error));
                    }
                }
            }
            // Validate derive attributes
            for derive_name in &s.derives {
                match derive_name.as_str() {
                    "Eq" | "Clone" | "Display" => {}
                    _ => {
                        self.error(
                            ErrorCode::E0319,
                            format!("unknown derive trait `{derive_name}`"),
                            item.span.clone(),
                        );
                    }
                }
            }
            self.structs.insert(
                s.name.clone(),
                StructInfo {
                    fields,
                    type_params: tp_names,
                    derives: s.derives.clone(),
                },
            );
        }

        // Pass 0b: register all enum definitions
        for item in &module.items {
            let Item::Enum(e) = &item.node else { continue };
            if self.enums.contains_key(&e.name) {
                self.error(
                    ErrorCode::E0307,
                    format!("enum `{}` is already defined", e.name),
                    item.span.clone(),
                );
                continue;
            }
            let tp_names: Vec<String> = e.type_param_names();
            let variants: Vec<(String, Vec<Ty>)> = e
                .variants
                .iter()
                .map(|v| {
                    let field_tys: Vec<Ty> = v
                        .fields
                        .iter()
                        .filter_map(|f| {
                            resolve_type_expr_with_params(
                                &f.node,
                                Some(&self.structs),
                                Some(&self.enums),
                                &tp_names,
                            )
                        })
                        .collect();
                    (v.name.clone(), field_tys)
                })
                .collect();
            self.enums.insert(
                e.name.clone(),
                EnumInfo {
                    variants,
                    type_params: tp_names,
                },
            );
        }

        // Pass 0c: register all trait definitions
        for item in &module.items {
            let Item::Trait(t) = &item.node else { continue };
            if self.traits.contains_key(&t.name) {
                self.error(
                    ErrorCode::E0309,
                    format!("trait `{}` is already defined", t.name),
                    item.span.clone(),
                );
                continue;
            }
            let mut trait_methods = Vec::new();
            for method in &t.methods {
                let mut params = Vec::new();
                for param in &method.params {
                    if param.name == "self" {
                        params.push(("self".to_string(), Ty::Error));
                    } else {
                        match resolve_type_expr(
                            &param.ty.node,
                            Some(&self.structs),
                            Some(&self.enums),
                        ) {
                            Some(ty) => params.push((param.name.clone(), ty)),
                            None => {
                                self.error(
                                    ErrorCode::E0323,
                                    format!("unknown type in trait method `{}`", method.name),
                                    param.ty.span.clone(),
                                );
                                params.push((param.name.clone(), Ty::Error));
                            }
                        }
                    }
                }
                let ret = if let Some(ret_type) = &method.return_type {
                    match resolve_type_expr(&ret_type.node, Some(&self.structs), Some(&self.enums))
                    {
                        Some(ty) => ty,
                        None => {
                            self.error(
                                ErrorCode::E0324,
                                format!("unknown return type in trait method `{}`", method.name),
                                ret_type.span.clone(),
                            );
                            Ty::Error
                        }
                    }
                } else {
                    Ty::Unit
                };
                trait_methods.push(TraitMethodInfo {
                    name: method.name.clone(),
                    params,
                    ret,
                    has_default: method.default_body.is_some(),
                });
            }
            self.traits.insert(
                t.name.clone(),
                TraitInfo {
                    methods: trait_methods,
                },
            );
        }

        // First pass: register all function signatures
        for item in &module.items {
            let Item::Function(f) = &item.node else {
                continue;
            };

            // Reject shadowing of builtin functions
            if Self::is_builtin_function(&f.name) {
                self.error(
                    ErrorCode::E0313,
                    format!("cannot redefine builtin function `{}`", f.name),
                    item.span.clone(),
                );
                continue;
            }

            if self.functions.contains_key(&f.name) {
                self.error(
                    ErrorCode::E0308,
                    format!("function `{}` is already defined", f.name),
                    item.span.clone(),
                );
                continue;
            }

            let tp_names: Vec<String> = f.type_params.iter().map(|tp| tp.name.clone()).collect();
            let mut tp_bounds: HashMap<String, Vec<String>> = HashMap::new();
            for tp in &f.type_params {
                if !tp.bounds.is_empty() {
                    tp_bounds.insert(tp.name.clone(), tp.bounds.clone());
                }
            }

            let mut params = Vec::new();
            for param in &f.params {
                match resolve_type_expr_with_params(
                    &param.ty.node,
                    Some(&self.structs),
                    Some(&self.enums),
                    &tp_names,
                ) {
                    Some(ty) => params.push((param.name.clone(), ty)),
                    None => {
                        if let TypeExpr::Named(name) = &param.ty.node {
                            self.error(
                                ErrorCode::E0305,
                                format!("unknown type `{name}`"),
                                param.ty.span.clone(),
                            );
                        }
                        params.push((param.name.clone(), Ty::Error));
                    }
                }
            }

            let ret = if let Some(ret_type) = &f.return_type {
                match resolve_type_expr_with_params(
                    &ret_type.node,
                    Some(&self.structs),
                    Some(&self.enums),
                    &tp_names,
                ) {
                    Some(ty) => ty,
                    None => {
                        if let TypeExpr::Named(name) = &ret_type.node {
                            self.error(
                                ErrorCode::E0324,
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

            self.functions.insert(
                f.name.clone(),
                FnSig {
                    type_params: tp_names,
                    type_param_bounds: tp_bounds,
                    params,
                    ret,
                    is_async: f.is_async,
                    is_tool: f.is_tool,
                    is_unsafe: f.is_unsafe,
                },
            );

            // Validate @test functions: must have no parameters and no return type
            if f.is_test {
                if !f.params.is_empty() {
                    self.error(
                        ErrorCode::E0504,
                        format!("@test function `{}` must have no parameters", f.name),
                        item.span.clone(),
                    );
                }
                if f.return_type.is_some() {
                    self.error(
                        ErrorCode::E0505,
                        format!("@test function `{}` must have no return type", f.name),
                        item.span.clone(),
                    );
                }
            }
        }

        // Pass 1b: register agent declarations
        for item in &module.items {
            let Item::Agent(agent) = &item.node else {
                continue;
            };
            if self.agents.contains_key(&agent.name) {
                self.error(
                    ErrorCode::E0312,
                    format!("agent `{}` is already defined", agent.name),
                    item.span.clone(),
                );
                continue;
            }
            self.agents.insert(
                agent.name.clone(),
                AgentInfo {
                    model: agent.model.clone(),
                    tools: agent.tools.clone(),
                    system_prompt: agent.system_prompt.clone(),
                },
            );
        }

        // Validate agent tool references point to actual tool functions
        for item in &module.items {
            if let Item::Agent(agent) = &item.node {
                for tool_name in &agent.tools {
                    match self.functions.get(tool_name) {
                        Some(sig) => {
                            if !sig.is_tool {
                                self.error(
                                    ErrorCode::E0322,
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
                                ErrorCode::E0321,
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

        // Pass 2: register impl block methods
        for item in &module.items {
            let Item::Impl(imp) = &item.node else {
                continue;
            };

            if !self.structs.contains_key(&imp.type_name) {
                self.error(
                    ErrorCode::E0305,
                    format!("impl block for undefined type `{}`", imp.type_name),
                    item.span.clone(),
                );
                continue;
            }

            // Collect method sigs first, then insert into self.methods
            // (avoids simultaneous mutable borrows of self)
            let mut new_methods: Vec<(String, FnSig, String)> = Vec::new(); // (method_name, sig, mangled_name)

            for method_spanned in &imp.methods {
                let method = &method_spanned.node;

                // Check duplicate via a temporary lookup
                let already_exists = self
                    .methods
                    .get(&imp.type_name)
                    .is_some_and(|m| m.contains_key(&method.name));
                if already_exists {
                    self.error(
                        ErrorCode::E0310,
                        format!(
                            "method `{}` is already defined for `{}`",
                            method.name, imp.type_name
                        ),
                        method_spanned.span.clone(),
                    );
                    continue;
                }

                let mut params = Vec::new();
                for param in &method.params {
                    if param.name == "self" {
                        params.push(("self".to_string(), Ty::Struct(imp.type_name.clone())));
                    } else {
                        match resolve_type_expr(
                            &param.ty.node,
                            Some(&self.structs),
                            Some(&self.enums),
                        ) {
                            Some(ty) => params.push((param.name.clone(), ty)),
                            None => {
                                if let TypeExpr::Named(name) = &param.ty.node {
                                    self.error(
                                        ErrorCode::E0305,
                                        format!("unknown type `{name}`"),
                                        param.ty.span.clone(),
                                    );
                                }
                                params.push((param.name.clone(), Ty::Error));
                            }
                        }
                    }
                }

                let ret = if let Some(ret_type) = &method.return_type {
                    match resolve_type_expr(&ret_type.node, Some(&self.structs), Some(&self.enums))
                    {
                        Some(ty) => ty,
                        None => {
                            if let TypeExpr::Named(name) = &ret_type.node {
                                self.error(
                                    ErrorCode::E0324,
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

                let mangled = format!("{}__{}", imp.type_name, method.name);
                let sig = FnSig {
                    type_params: Vec::new(),
                    type_param_bounds: HashMap::new(),
                    params,
                    ret,
                    is_async: false,
                    is_tool: false,
                    is_unsafe: false,
                };
                new_methods.push((method.name.clone(), sig, mangled));
            }

            // Now insert all collected methods
            let type_methods = self.methods.entry(imp.type_name.clone()).or_default();
            let method_names: Vec<String> = new_methods.iter().map(|(n, _, _)| n.clone()).collect();
            for (method_name, sig, mangled) in new_methods {
                type_methods.insert(method_name, sig.clone());
                self.functions.insert(mangled, sig);
            }

            // Validate trait impl: check all required methods are provided with correct signatures
            if let Some(trait_name) = &imp.trait_name {
                if let Some(trait_info) = self.traits.get(trait_name).cloned() {
                    for trait_method in &trait_info.methods {
                        if !method_names.contains(&trait_method.name) {
                            if trait_method.has_default {
                                // Auto-provide the default method: register its signature
                                let mangled = format!("{}__{}", imp.type_name, trait_method.name);
                                let mut params = trait_method.params.clone();
                                // Replace self's Ty::Error with the actual struct type
                                for p in &mut params {
                                    if p.0 == "self" {
                                        p.1 = Ty::Struct(imp.type_name.clone());
                                    }
                                }
                                let sig = FnSig {
                                    type_params: Vec::new(),
                                    type_param_bounds: HashMap::new(),
                                    params,
                                    ret: trait_method.ret.clone(),
                                    is_async: false,
                                    is_tool: false,
                                    is_unsafe: false,
                                };
                                let type_methods =
                                    self.methods.entry(imp.type_name.clone()).or_default();
                                type_methods.insert(trait_method.name.clone(), sig.clone());
                                self.functions.insert(mangled, sig);
                            } else {
                                self.error(ErrorCode::E0509,
                                    format!(
                                        "trait `{trait_name}` requires method `{}` but it is not implemented for `{}`",
                                        trait_method.name, imp.type_name
                                    ),
                                    item.span.clone(),
                                );
                            }
                        } else {
                            // Check return type matches
                            let mangled = format!("{}__{}", imp.type_name, trait_method.name);
                            if let Some(impl_sig) = self.functions.get(&mangled) {
                                if !trait_method.ret.is_error()
                                    && !impl_sig.ret.is_error()
                                    && trait_method.ret != impl_sig.ret
                                {
                                    self.error(ErrorCode::E0510,
                                        format!(
                                            "method `{}` in impl of `{trait_name}` for `{}` has return type `{}`, expected `{}`",
                                            trait_method.name, imp.type_name, impl_sig.ret, trait_method.ret
                                        ),
                                        item.span.clone(),
                                    );
                                }
                            }
                        }
                    }
                    // Record this trait impl
                    self.trait_impls
                        .entry(imp.type_name.clone())
                        .or_default()
                        .push(trait_name.clone());
                } else {
                    self.error(
                        ErrorCode::E0304,
                        format!("undefined trait `{trait_name}`"),
                        item.span.clone(),
                    );
                }
            }
        }

        // Auto-register Display trait impl for structs with @derive(Display)
        for item in &module.items {
            let Item::Struct(s) = &item.node else {
                continue;
            };
            if s.derives.contains(&"Display".to_string()) {
                // Only add if not already explicitly implemented
                let already_impl = self
                    .trait_impls
                    .get(&s.name)
                    .is_some_and(|impls| impls.contains(&"Display".to_string()));
                if !already_impl {
                    // Register the to_string method signature
                    let mangled = format!("{}__{}", s.name, "to_string");
                    let sig = FnSig {
                        type_params: Vec::new(),
                        type_param_bounds: HashMap::new(),
                        params: vec![("self".to_string(), Ty::Struct(s.name.clone()))],
                        ret: Ty::Str,
                        is_async: false,
                        is_tool: false,
                        is_unsafe: false,
                    };
                    let type_methods = self.methods.entry(s.name.clone()).or_default();
                    type_methods.insert("to_string".to_string(), sig.clone());
                    self.functions.insert(mangled, sig);
                    self.trait_impls
                        .entry(s.name.clone())
                        .or_default()
                        .push("Display".to_string());
                }
            }
        }

        // Register constants
        for item in &module.items {
            let Item::Const(c) = &item.node else { continue };
            if self.constants.contains_key(&c.name) {
                self.error(
                    ErrorCode::E0311,
                    format!("constant `{}` is already defined", c.name),
                    item.span.clone(),
                );
                continue;
            }
            // Check that the value is a literal
            let ty = match &c.value.node {
                Expr::IntLit(_) => Ty::I64,
                Expr::FloatLit(_) => Ty::F64,
                Expr::BoolLit(_) => Ty::Bool,
                Expr::StringLit(_) => Ty::Str,
                Expr::UnaryOp {
                    op: UnaryOp::Neg,
                    expr,
                } => match &expr.node {
                    Expr::IntLit(_) => Ty::I64,
                    Expr::FloatLit(_) => Ty::F64,
                    _ => {
                        self.error(
                            ErrorCode::E0506,
                            format!("constant `{}` must be a literal value", c.name),
                            c.value.span.clone(),
                        );
                        Ty::Error
                    }
                },
                _ => {
                    self.error(
                        ErrorCode::E0506,
                        format!("constant `{}` must be a literal value", c.name),
                        c.value.span.clone(),
                    );
                    Ty::Error
                }
            };
            self.constants.insert(c.name.clone(), ty);
        }

        // Check for main (not required in test mode)
        if !self.test_mode && !self.functions.contains_key("main") {
            let span = if module.items.is_empty() {
                0..0
            } else {
                module.items.last().unwrap().span.clone()
            };
            self.error(
                ErrorCode::E0314,
                "no `main` function found".to_string(),
                span,
            );
        }

        // Second pass: check function bodies (skip those not registered, e.g. builtin shadows)
        for item in &module.items {
            let Item::Function(f) = &item.node else {
                continue;
            };
            if self.functions.contains_key(&f.name) {
                self.check_function(f);
            }
        }

        // Check impl method bodies
        for item in &module.items {
            let Item::Impl(imp) = &item.node else {
                continue;
            };
            if !self.structs.contains_key(&imp.type_name) {
                continue;
            }
            for method_spanned in &imp.methods {
                let method = &method_spanned.node;
                let mangled = format!("{}__{}", imp.type_name, method.name);
                if self.functions.contains_key(&mangled) {
                    self.check_function_with_name(method, &mangled);
                }
            }
        }
    }

    /// Substitute type parameters using a substitution map
    fn substitute_ty(&self, ty: &Ty, subs: &HashMap<String, Ty>) -> Ty {
        match ty {
            Ty::TypeParam(name) => subs.get(name).cloned().unwrap_or_else(|| ty.clone()),
            other => other.clone(),
        }
    }

    /// Get the concrete return type of a function given substitutions
    fn substitute_return_type(&self, sig: &FnSig, subs: &HashMap<String, Ty>) -> Ty {
        self.substitute_ty(&sig.ret, subs)
    }

    /// Inject module-level constants into the current scope as immutable variables.
    fn inject_constants(&mut self) {
        let consts: Vec<(String, Ty)> = self
            .constants
            .iter()
            .map(|(n, t)| (n.clone(), t.clone()))
            .collect();
        for (name, ty) in consts {
            self.define_var(&name, VarInfo { ty, mutable: false }, &(0..0));
        }
    }

    fn check_function(&mut self, f: &FnDef) {
        let sig = self.functions.get(&f.name).cloned().unwrap();

        // Skip body checking for generic functions — their bodies
        // contain type parameters that aren't concrete types.
        // Type safety is enforced at the call site via inference.
        if !sig.type_params.is_empty() {
            return;
        }

        self.current_return_type = sig.ret.clone();
        let prev_unsafe = self.in_unsafe_context;
        self.in_unsafe_context = f.is_unsafe;

        self.push_scope();

        // Inject module-level constants
        self.inject_constants();

        // Define parameters
        for (name, ty) in &sig.params {
            self.define_var(
                name,
                VarInfo {
                    ty: ty.clone(),
                    mutable: false,
                },
                &(0..0),
            );
        }

        // Check body
        let body_ty = self.check_expr(&f.body);

        // Check return type matches
        if !sig.ret.is_error()
            && !body_ty.is_error()
            && sig.ret != Ty::Unit
            && !types_compatible(&sig.ret, &body_ty)
        {
            self.error(
                ErrorCode::E0109,
                format!(
                    "function `{}` should return `{}` but body returns `{}`",
                    f.name, sig.ret, body_ty
                ),
                f.body.span.clone(),
            );
        }

        self.pop_scope();
        self.in_unsafe_context = prev_unsafe;
    }

    /// Check a function body using a different name for looking up its signature (for methods).
    fn check_function_with_name(&mut self, f: &FnDef, sig_name: &str) {
        let sig = self.functions.get(sig_name).cloned().unwrap();
        self.current_return_type = sig.ret.clone();

        self.push_scope();

        // Inject module-level constants
        self.inject_constants();

        for (name, ty) in &sig.params {
            self.define_var(
                name,
                VarInfo {
                    ty: ty.clone(),
                    mutable: false,
                },
                &(0..0),
            );
        }

        let body_ty = self.check_expr(&f.body);

        if !sig.ret.is_error()
            && !body_ty.is_error()
            && sig.ret != Ty::Unit
            && !types_compatible(&sig.ret, &body_ty)
        {
            self.error(
                ErrorCode::E0109,
                format!(
                    "method `{}` should return `{}` but body returns `{}`",
                    f.name, sig.ret, body_ty
                ),
                f.body.span.clone(),
            );
        }

        self.pop_scope();
    }

    // === Expression type checking ===

    fn check_expr(&mut self, expr: &Spanned<Expr>) -> Ty {
        self.expr_depth += 1;
        if self.expr_depth > MAX_EXPR_DEPTH {
            self.error(
                ErrorCode::E0136,
                "expression nesting too deep (possible infinite recursion)".to_string(),
                expr.span.clone(),
            );
            self.expr_depth -= 1;
            return Ty::Error;
        }
        let result = self.check_expr_inner(expr);
        self.expr_depth -= 1;
        result
    }

    fn check_expr_inner(&mut self, expr: &Spanned<Expr>) -> Ty {
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
                        ErrorCode::E0300,
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
                        // String concatenation: str + str
                        if *op == BinOp::Add && lhs == Ty::Str && rhs == Ty::Str {
                            return Ty::Str;
                        }
                        // Reject mixed str + non-str in arithmetic — use to_str() or string interpolation
                        if *op == BinOp::Add {
                            if lhs == Ty::Str && rhs != Ty::Str {
                                self.error(
                                    ErrorCode::E0102,
                                    format!("cannot add `str` and `{rhs}` — use to_str() or string interpolation"),
                                    expr.span.clone(),
                                );
                                return Ty::Error;
                            }
                            if rhs == Ty::Str && lhs != Ty::Str {
                                self.error(
                                    ErrorCode::E0102,
                                    format!("cannot add `{lhs}` and `str` — use to_str() or string interpolation"),
                                    expr.span.clone(),
                                );
                                return Ty::Error;
                            }
                        }
                        if !lhs.is_numeric() {
                            self.error(
                                ErrorCode::E0101,
                                format!("cannot perform arithmetic on `{lhs}`"),
                                left.span.clone(),
                            );
                            return Ty::Error;
                        }
                        if lhs != rhs {
                            self.error(
                                ErrorCode::E0102,
                                format!("mismatched types in arithmetic: `{lhs}` and `{rhs}`"),
                                expr.span.clone(),
                            );
                            return Ty::Error;
                        }
                        lhs
                    }
                    BinOp::Eq
                    | BinOp::NotEq
                    | BinOp::Less
                    | BinOp::LessEq
                    | BinOp::Greater
                    | BinOp::GreaterEq => {
                        if lhs != rhs {
                            self.error(
                                ErrorCode::E0103,
                                format!("cannot compare `{lhs}` with `{rhs}`"),
                                expr.span.clone(),
                            );
                            return Ty::Error;
                        }
                        // Struct equality requires @derive(Eq)
                        if let Ty::Struct(ref struct_name) = lhs {
                            if matches!(op, BinOp::Eq | BinOp::NotEq) {
                                if let Some(info) = self.structs.get(struct_name) {
                                    if !info.derives.contains(&"Eq".to_string()) {
                                        self.error(ErrorCode::E0128,
                                            format!("cannot compare struct `{struct_name}` with `==`/`!=` without `@derive(Eq)`"),
                                            expr.span.clone(),
                                        );
                                        return Ty::Error;
                                    }
                                }
                            } else {
                                self.error(
                                    ErrorCode::E0129,
                                    format!(
                                        "cannot use ordering comparison on struct `{struct_name}`"
                                    ),
                                    expr.span.clone(),
                                );
                                return Ty::Error;
                            }
                        }
                        Ty::Bool
                    }
                    BinOp::And | BinOp::Or => {
                        if lhs != Ty::Bool {
                            self.error(
                                ErrorCode::E0104,
                                format!("expected `bool` in logical operation, found `{lhs}`"),
                                left.span.clone(),
                            );
                        }
                        if rhs != Ty::Bool {
                            self.error(
                                ErrorCode::E0104,
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
                                ErrorCode::E0105,
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
                                ErrorCode::E0106,
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
                    if name == "print" {
                        if args.len() > 1 {
                            self.error(
                                ErrorCode::E0513,
                                format!("print() takes at most 1 argument, got {}", args.len()),
                                callee.span.clone(),
                            );
                        }
                        for arg in args {
                            self.check_expr(arg);
                        }
                        return Ty::Unit;
                    }
                    if name == "panic" {
                        if args.len() > 1 {
                            self.error(
                                ErrorCode::E0513,
                                format!("panic() takes at most 1 argument, got {}", args.len()),
                                callee.span.clone(),
                            );
                        }
                        for arg in args {
                            self.check_expr(arg);
                        }
                        return Ty::Unit;
                    }
                    if name == "assert" {
                        if args.is_empty() {
                            self.error(
                                ErrorCode::E0513,
                                "assert() requires at least one argument".to_string(),
                                callee.span.clone(),
                            );
                        } else if args.len() > 2 {
                            self.error(
                                ErrorCode::E0513,
                                format!("assert() takes at most 2 arguments, got {}", args.len()),
                                callee.span.clone(),
                            );
                        }
                        if !args.is_empty() {
                            let cond_ty = self.check_expr(&args[0]);
                            if !cond_ty.is_error() && cond_ty != Ty::Bool {
                                self.error(
                                    ErrorCode::E0133,
                                    format!("assert() condition must be `bool`, found `{cond_ty}`"),
                                    args[0].span.clone(),
                                );
                            }
                            // Optional message argument
                            if args.len() > 1 {
                                self.check_expr(&args[1]);
                            }
                            // Type-check remaining args even if too many
                            for arg in args.iter().skip(2) {
                                self.check_expr(arg);
                            }
                        }
                        return Ty::Unit;
                    }
                    if name == "assert_eq" || name == "assert_ne" {
                        if args.len() != 2 {
                            self.error(
                                ErrorCode::E0100,
                                format!("{name}() takes exactly 2 arguments, got {}", args.len()),
                                callee.span.clone(),
                            );
                        }
                        if args.len() >= 2 {
                            let left_ty = self.check_expr(&args[0]);
                            let right_ty = self.check_expr(&args[1]);
                            if !left_ty.is_error()
                                && !right_ty.is_error()
                                && !types_compatible(&left_ty, &right_ty)
                                && left_ty != right_ty
                            {
                                self.error(ErrorCode::E0100,
                                    format!("{name}() arguments must be the same type: left is `{left_ty}`, right is `{right_ty}`"),
                                    callee.span.clone(),
                                );
                            }
                        }
                        for arg in args {
                            self.check_expr(arg);
                        }
                        return Ty::Unit;
                    }
                    if name == "len" {
                        if args.len() != 1 {
                            self.error(
                                ErrorCode::E0513,
                                format!("len() takes exactly 1 argument, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let arg_ty = self.check_expr(&args[0]);
                        match &arg_ty {
                            Ty::Array(_) => return Ty::I64,
                            Ty::Str => return Ty::I64,
                            _ if arg_ty.is_error() => return Ty::Error,
                            _ => {
                                self.error(
                                    ErrorCode::E0133,
                                    format!("len() expects array or string, found `{arg_ty}`"),
                                    args[0].span.clone(),
                                );
                                return Ty::Error;
                            }
                        }
                    }

                    if name == "abs" {
                        if args.len() != 1 {
                            self.error(
                                ErrorCode::E0513,
                                format!("abs() takes exactly 1 argument, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let arg_ty = self.check_expr(&args[0]);
                        if !arg_ty.is_error() && !arg_ty.is_integer() {
                            self.error(
                                ErrorCode::E0133,
                                format!("abs() expects integer, found `{arg_ty}`"),
                                args[0].span.clone(),
                            );
                        }
                        return Ty::I64;
                    }
                    if name == "min" || name == "max" {
                        if args.len() != 2 {
                            self.error(
                                ErrorCode::E0100,
                                format!("{}() takes exactly 2 arguments, got {}", name, args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let a_ty = self.check_expr(&args[0]);
                        let b_ty = self.check_expr(&args[1]);
                        if !a_ty.is_error() && !a_ty.is_integer() {
                            self.error(
                                ErrorCode::E0100,
                                format!("{}() expects integers, found `{a_ty}`", name),
                                args[0].span.clone(),
                            );
                        }
                        if !b_ty.is_error() && !b_ty.is_integer() {
                            self.error(
                                ErrorCode::E0100,
                                format!("{}() expects integers, found `{b_ty}`", name),
                                args[1].span.clone(),
                            );
                        }
                        return Ty::I64;
                    }
                    if name == "to_str" {
                        if args.len() != 1 {
                            self.error(
                                ErrorCode::E0513,
                                format!("to_str() takes exactly 1 argument, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        self.check_expr(&args[0]);
                        return Ty::Str;
                    }

                    // ── Stdlib string functions ──────────────────────
                    if name == "split" {
                        if args.len() != 2 {
                            self.error(
                                ErrorCode::E0513,
                                format!("split() takes exactly 2 arguments, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let s_ty = self.check_expr(&args[0]);
                        let sep_ty = self.check_expr(&args[1]);
                        if !s_ty.is_error() && s_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0133,
                                format!("split() first argument must be str, found `{s_ty}`"),
                                args[0].span.clone(),
                            );
                        }
                        if !sep_ty.is_error() && sep_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0133,
                                format!("split() second argument must be str, found `{sep_ty}`"),
                                args[1].span.clone(),
                            );
                        }
                        return Ty::Array(Box::new(Ty::Str));
                    }
                    if name == "trim" {
                        if args.len() != 1 {
                            self.error(
                                ErrorCode::E0513,
                                format!("trim() takes exactly 1 argument, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let s_ty = self.check_expr(&args[0]);
                        if !s_ty.is_error() && s_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0100,
                                format!("trim() expects str, found `{s_ty}`"),
                                args[0].span.clone(),
                            );
                        }
                        return Ty::Str;
                    }
                    if name == "upper" {
                        if args.len() != 1 {
                            self.error(
                                ErrorCode::E0100,
                                format!("upper() takes exactly 1 argument, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let s_ty = self.check_expr(&args[0]);
                        if !s_ty.is_error() && s_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0100,
                                format!("upper() expects str, found `{s_ty}`"),
                                args[0].span.clone(),
                            );
                        }
                        return Ty::Str;
                    }
                    if name == "lower" {
                        if args.len() != 1 {
                            self.error(
                                ErrorCode::E0100,
                                format!("lower() takes exactly 1 argument, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let s_ty = self.check_expr(&args[0]);
                        if !s_ty.is_error() && s_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0100,
                                format!("lower() expects str, found `{s_ty}`"),
                                args[0].span.clone(),
                            );
                        }
                        return Ty::Str;
                    }
                    if name == "starts_with" {
                        if args.len() != 2 {
                            self.error(
                                ErrorCode::E0513,
                                format!(
                                    "starts_with() takes exactly 2 arguments, got {}",
                                    args.len()
                                ),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let s_ty = self.check_expr(&args[0]);
                        let prefix_ty = self.check_expr(&args[1]);
                        if !s_ty.is_error() && s_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0133,
                                format!("starts_with() first argument must be str, found `{s_ty}`"),
                                args[0].span.clone(),
                            );
                        }
                        if !prefix_ty.is_error() && prefix_ty != Ty::Str {
                            self.error(ErrorCode::E0133, format!("starts_with() second argument must be str, found `{prefix_ty}`"), args[1].span.clone());
                        }
                        return Ty::Bool;
                    }
                    if name == "ends_with" {
                        if args.len() != 2 {
                            self.error(
                                ErrorCode::E0513,
                                format!(
                                    "ends_with() takes exactly 2 arguments, got {}",
                                    args.len()
                                ),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let s_ty = self.check_expr(&args[0]);
                        let suffix_ty = self.check_expr(&args[1]);
                        if !s_ty.is_error() && s_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0133,
                                format!("ends_with() first argument must be str, found `{s_ty}`"),
                                args[0].span.clone(),
                            );
                        }
                        if !suffix_ty.is_error() && suffix_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0133,
                                format!(
                                    "ends_with() second argument must be str, found `{suffix_ty}`"
                                ),
                                args[1].span.clone(),
                            );
                        }
                        return Ty::Bool;
                    }
                    if name == "replace" {
                        if args.len() != 3 {
                            self.error(
                                ErrorCode::E0513,
                                format!("replace() takes exactly 3 arguments, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let s_ty = self.check_expr(&args[0]);
                        let from_ty = self.check_expr(&args[1]);
                        let to_ty = self.check_expr(&args[2]);
                        if !s_ty.is_error() && s_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0133,
                                format!("replace() first argument must be str, found `{s_ty}`"),
                                args[0].span.clone(),
                            );
                        }
                        if !from_ty.is_error() && from_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0133,
                                format!("replace() second argument must be str, found `{from_ty}`"),
                                args[1].span.clone(),
                            );
                        }
                        if !to_ty.is_error() && to_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0133,
                                format!("replace() third argument must be str, found `{to_ty}`"),
                                args[2].span.clone(),
                            );
                        }
                        return Ty::Str;
                    }
                    if name == "char_at" {
                        if args.len() != 2 {
                            self.error(
                                ErrorCode::E0513,
                                format!("char_at() takes exactly 2 arguments, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let s_ty = self.check_expr(&args[0]);
                        let idx_ty = self.check_expr(&args[1]);
                        if !s_ty.is_error() && s_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0133,
                                format!("char_at() first argument must be str, found `{s_ty}`"),
                                args[0].span.clone(),
                            );
                        }
                        if !idx_ty.is_error() && !idx_ty.is_integer() {
                            self.error(
                                ErrorCode::E0133,
                                format!(
                                    "char_at() second argument must be integer, found `{idx_ty}`"
                                ),
                                args[1].span.clone(),
                            );
                        }
                        return Ty::Str;
                    }

                    if name == "contains" {
                        if args.len() != 2 {
                            self.error(
                                ErrorCode::E0513,
                                format!("contains() takes exactly 2 arguments, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let s_ty = self.check_expr(&args[0]);
                        let sub_ty = self.check_expr(&args[1]);
                        if !s_ty.is_error() && s_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0133,
                                format!("contains() first argument must be str, found `{s_ty}`"),
                                args[0].span.clone(),
                            );
                        }
                        if !sub_ty.is_error() && sub_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0133,
                                format!("contains() second argument must be str, found `{sub_ty}`"),
                                args[1].span.clone(),
                            );
                        }
                        return Ty::Bool;
                    }
                    if name == "index_of" {
                        if args.len() != 2 {
                            self.error(
                                ErrorCode::E0100,
                                format!("index_of() takes exactly 2 arguments, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let s_ty = self.check_expr(&args[0]);
                        let sub_ty = self.check_expr(&args[1]);
                        if !s_ty.is_error() && s_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0100,
                                format!("index_of() first argument must be str, found `{s_ty}`"),
                                args[0].span.clone(),
                            );
                        }
                        if !sub_ty.is_error() && sub_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0100,
                                format!("index_of() second argument must be str, found `{sub_ty}`"),
                                args[1].span.clone(),
                            );
                        }
                        return Ty::I64;
                    }
                    if name == "join" {
                        if args.len() != 2 {
                            self.error(
                                ErrorCode::E0513,
                                format!("join() takes exactly 2 arguments, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let arr_ty = self.check_expr(&args[0]);
                        let sep_ty = self.check_expr(&args[1]);
                        if !arr_ty.is_error() && arr_ty != Ty::Array(Box::new(Ty::Str)) {
                            self.error(
                                ErrorCode::E0133,
                                format!("join() first argument must be [str], found `{arr_ty}`"),
                                args[0].span.clone(),
                            );
                        }
                        if !sep_ty.is_error() && sep_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0133,
                                format!("join() second argument must be str, found `{sep_ty}`"),
                                args[1].span.clone(),
                            );
                        }
                        return Ty::Str;
                    }
                    if name == "repeat" {
                        if args.len() != 2 {
                            self.error(
                                ErrorCode::E0100,
                                format!("repeat() takes exactly 2 arguments, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let s_ty = self.check_expr(&args[0]);
                        let n_ty = self.check_expr(&args[1]);
                        if !s_ty.is_error() && s_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0100,
                                format!("repeat() first argument must be str, found `{s_ty}`"),
                                args[0].span.clone(),
                            );
                        }
                        if !n_ty.is_error() && !n_ty.is_integer() {
                            self.error(
                                ErrorCode::E0100,
                                format!("repeat() second argument must be integer, found `{n_ty}`"),
                                args[1].span.clone(),
                            );
                        }
                        return Ty::Str;
                    }

                    // ── Stdlib I/O functions ─────────────────────────
                    if name == "read_line" {
                        if !args.is_empty() {
                            self.error(
                                ErrorCode::E0100,
                                format!("read_line() takes 0 arguments, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        return Ty::Str;
                    }
                    if name == "read_file" {
                        if args.len() != 1 {
                            self.error(
                                ErrorCode::E0513,
                                format!("read_file() takes exactly 1 argument, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let path_ty = self.check_expr(&args[0]);
                        if !path_ty.is_error() && path_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0100,
                                format!("read_file() expects str, found `{path_ty}`"),
                                args[0].span.clone(),
                            );
                        }
                        return Ty::Str;
                    }
                    if name == "write_file" {
                        if args.len() != 2 {
                            self.error(
                                ErrorCode::E0513,
                                format!(
                                    "write_file() takes exactly 2 arguments, got {}",
                                    args.len()
                                ),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let path_ty = self.check_expr(&args[0]);
                        let content_ty = self.check_expr(&args[1]);
                        if !path_ty.is_error() && path_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0133,
                                format!(
                                    "write_file() first argument must be str, found `{path_ty}`"
                                ),
                                args[0].span.clone(),
                            );
                        }
                        if !content_ty.is_error() && content_ty != Ty::Str {
                            self.error(ErrorCode::E0133, format!("write_file() second argument must be str, found `{content_ty}`"), args[1].span.clone());
                        }
                        return Ty::Unit;
                    }

                    // ── Stdlib math functions ────────────────────────
                    if name == "pow" {
                        if args.len() != 2 {
                            self.error(
                                ErrorCode::E0100,
                                format!("pow() takes exactly 2 arguments, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let base_ty = self.check_expr(&args[0]);
                        let exp_ty = self.check_expr(&args[1]);
                        if !base_ty.is_error() && !base_ty.is_integer() {
                            self.error(
                                ErrorCode::E0100,
                                format!("pow() first argument must be integer, found `{base_ty}`"),
                                args[0].span.clone(),
                            );
                        }
                        if !exp_ty.is_error() && !exp_ty.is_integer() {
                            self.error(
                                ErrorCode::E0100,
                                format!("pow() second argument must be integer, found `{exp_ty}`"),
                                args[1].span.clone(),
                            );
                        }
                        return Ty::I64;
                    }
                    if name == "sqrt" {
                        if args.len() != 1 {
                            self.error(
                                ErrorCode::E0100,
                                format!("sqrt() takes exactly 1 argument, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let x_ty = self.check_expr(&args[0]);
                        if !x_ty.is_error() && x_ty != Ty::F64 && x_ty != Ty::F32 {
                            self.error(
                                ErrorCode::E0100,
                                format!("sqrt() expects float, found `{x_ty}`"),
                                args[0].span.clone(),
                            );
                        }
                        return Ty::F64;
                    }

                    // sleep(ms: i64) -> () — sleep the current thread
                    if name == "sleep" {
                        if args.len() != 1 {
                            self.error(
                                ErrorCode::E0100,
                                format!("sleep() takes exactly 1 argument, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let ms_ty = self.check_expr(&args[0]);
                        if !ms_ty.is_error() && !ms_ty.is_integer() {
                            self.error(
                                ErrorCode::E0100,
                                format!("sleep() expects integer (milliseconds), found `{ms_ty}`"),
                                args[0].span.clone(),
                            );
                        }
                        return Ty::Unit;
                    }

                    // ── HTTP builtins ───────────────────────────────
                    // http_get(url: str) -> str
                    if name == "http_get" {
                        if args.len() != 1 {
                            self.error(
                                ErrorCode::E0513,
                                format!("http_get() takes exactly 1 argument, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let url_ty = self.check_expr(&args[0]);
                        if !url_ty.is_error() && url_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0100,
                                format!("http_get() expects str, found `{url_ty}`"),
                                args[0].span.clone(),
                            );
                        }
                        return Ty::Str;
                    }
                    // http_post(url: str, body: str) -> str
                    if name == "http_post" {
                        if args.len() != 2 {
                            self.error(
                                ErrorCode::E0513,
                                format!(
                                    "http_post() takes exactly 2 arguments, got {}",
                                    args.len()
                                ),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let url_ty = self.check_expr(&args[0]);
                        let body_ty = self.check_expr(&args[1]);
                        if !url_ty.is_error() && url_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0133,
                                format!("http_post() first argument must be str, found `{url_ty}`"),
                                args[0].span.clone(),
                            );
                        }
                        if !body_ty.is_error() && body_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0133,
                                format!(
                                    "http_post() second argument must be str, found `{body_ty}`"
                                ),
                                args[1].span.clone(),
                            );
                        }
                        return Ty::Str;
                    }

                    // ── HTTP server builtins ──────────────────────────
                    // http_server(port: i64) -> i64
                    if name == "http_server" {
                        if args.len() != 1 {
                            self.error(
                                ErrorCode::E0100,
                                format!(
                                    "http_server() takes exactly 1 argument, got {}",
                                    args.len()
                                ),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let port_ty = self.check_expr(&args[0]);
                        if !port_ty.is_error() && !port_ty.is_integer() {
                            self.error(
                                ErrorCode::E0100,
                                format!("http_server() expects integer port, found `{port_ty}`"),
                                args[0].span.clone(),
                            );
                        }
                        return Ty::I64;
                    }
                    // route(server: i64, method: str, path: str, handler: fn(str) -> str)
                    if name == "route" {
                        if args.len() != 4 {
                            self.error(
                                ErrorCode::E0513,
                                format!("route() takes exactly 4 arguments, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let server_ty = self.check_expr(&args[0]);
                        let method_ty = self.check_expr(&args[1]);
                        let path_ty = self.check_expr(&args[2]);
                        // Set hint so closure param types can be inferred
                        self.closure_param_hint = Some(vec![Ty::Str]);
                        let handler_ty = self.check_expr(&args[3]);
                        if !server_ty.is_error() && !server_ty.is_integer() {
                            self.error(ErrorCode::E0133, format!("route() first argument must be server id (i64), found `{server_ty}`"), args[0].span.clone());
                        }
                        if !method_ty.is_error() && method_ty != Ty::Str {
                            self.error(ErrorCode::E0133, format!("route() second argument must be str (HTTP method), found `{method_ty}`"), args[1].span.clone());
                        }
                        if !path_ty.is_error() && path_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0133,
                                format!(
                                    "route() third argument must be str (path), found `{path_ty}`"
                                ),
                                args[2].span.clone(),
                            );
                        }
                        match &handler_ty {
                            Ty::Fn(params, ret) => {
                                if params.len() != 1 {
                                    self.error(ErrorCode::E0133, format!("route() handler must take 1 parameter (request), takes {}", params.len()), args[3].span.clone());
                                } else if !params[0].is_error() && params[0] != Ty::Str {
                                    self.error(
                                        ErrorCode::E0133,
                                        format!(
                                            "route() handler parameter must be str, found `{}`",
                                            params[0]
                                        ),
                                        args[3].span.clone(),
                                    );
                                }
                                if !ret.is_error() && **ret != Ty::Str {
                                    self.error(
                                        ErrorCode::E0100,
                                        format!(
                                            "route() handler must return str, returns `{}`",
                                            ret
                                        ),
                                        args[3].span.clone(),
                                    );
                                }
                            }
                            _ if handler_ty.is_error() => {}
                            _ => {
                                self.error(ErrorCode::E0133, format!("route() fourth argument must be a function, found `{handler_ty}`"), args[3].span.clone());
                            }
                        }
                        return Ty::Unit;
                    }
                    // http_listen(server: i64) -> ()
                    if name == "http_listen" {
                        if args.len() != 1 {
                            self.error(
                                ErrorCode::E0513,
                                format!(
                                    "http_listen() takes exactly 1 argument, got {}",
                                    args.len()
                                ),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let server_ty = self.check_expr(&args[0]);
                        if !server_ty.is_error() && !server_ty.is_integer() {
                            self.error(
                                ErrorCode::E0100,
                                format!(
                                    "http_listen() expects server id (i64), found `{server_ty}`"
                                ),
                                args[0].span.clone(),
                            );
                        }
                        return Ty::Unit;
                    }
                    // respond(status: i64, body: str) -> str
                    if name == "respond" {
                        if args.len() != 2 {
                            self.error(
                                ErrorCode::E0513,
                                format!("respond() takes exactly 2 arguments, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let status_ty = self.check_expr(&args[0]);
                        let body_ty = self.check_expr(&args[1]);
                        if !status_ty.is_error() && !status_ty.is_integer() {
                            self.error(ErrorCode::E0133, format!("respond() first argument must be integer status code, found `{status_ty}`"), args[0].span.clone());
                        }
                        if !body_ty.is_error() && body_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0133,
                                format!("respond() second argument must be str, found `{body_ty}`"),
                                args[1].span.clone(),
                            );
                        }
                        return Ty::Str;
                    }
                    // request_body(req: str) -> str
                    if name == "request_body" {
                        if args.len() != 1 {
                            self.error(
                                ErrorCode::E0100,
                                format!(
                                    "request_body() takes exactly 1 argument, got {}",
                                    args.len()
                                ),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let req_ty = self.check_expr(&args[0]);
                        if !req_ty.is_error() && req_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0100,
                                format!("request_body() expects str, found `{req_ty}`"),
                                args[0].span.clone(),
                            );
                        }
                        return Ty::Str;
                    }

                    // ── JSON builtins ───────────────────────────────
                    // json_get(json: str, key: str) -> str
                    if name == "json_get" {
                        if args.len() != 2 {
                            self.error(
                                ErrorCode::E0513,
                                format!("json_get() takes exactly 2 arguments, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let json_ty = self.check_expr(&args[0]);
                        let key_ty = self.check_expr(&args[1]);
                        if !json_ty.is_error() && json_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0133,
                                format!("json_get() first argument must be str, found `{json_ty}`"),
                                args[0].span.clone(),
                            );
                        }
                        if !key_ty.is_error() && key_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0133,
                                format!("json_get() second argument must be str, found `{key_ty}`"),
                                args[1].span.clone(),
                            );
                        }
                        return Ty::Str;
                    }
                    // json_stringify(key: str, value: str) -> str
                    if name == "json_stringify" {
                        if args.len() != 2 {
                            self.error(
                                ErrorCode::E0513,
                                format!(
                                    "json_stringify() takes exactly 2 arguments, got {}",
                                    args.len()
                                ),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let key_ty = self.check_expr(&args[0]);
                        let value_ty = self.check_expr(&args[1]);
                        if !key_ty.is_error() && key_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0133,
                                format!(
                                    "json_stringify() first argument must be str, found `{key_ty}`"
                                ),
                                args[0].span.clone(),
                            );
                        }
                        if !value_ty.is_error() && value_ty != Ty::Str {
                            self.error(ErrorCode::E0133, format!("json_stringify() second argument must be str, found `{value_ty}`"), args[1].span.clone());
                        }
                        return Ty::Str;
                    }
                    // to_json(val: any) -> str
                    if name == "to_json" {
                        if args.len() != 1 {
                            self.error(
                                ErrorCode::E0100,
                                format!("to_json() takes exactly 1 argument, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let _val_ty = self.check_expr(&args[0]);
                        return Ty::Str;
                    }
                    // to_json_array(arr: [any]) -> str
                    if name == "to_json_array" {
                        if args.len() != 1 {
                            self.error(
                                ErrorCode::E0100,
                                format!(
                                    "to_json_array() takes exactly 1 argument, got {}",
                                    args.len()
                                ),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let val_ty = self.check_expr(&args[0]);
                        if !val_ty.is_error() && !matches!(val_ty, Ty::Array(_)) {
                            self.error(
                                ErrorCode::E0100,
                                format!(
                                    "to_json_array() argument must be an array, found `{val_ty}`"
                                ),
                                args[0].span.clone(),
                            );
                        }
                        return Ty::Str;
                    }

                    // ── Channel builtins ───────────────────────────────
                    // channel() -> i64 (channel pointer)
                    if name == "channel" {
                        if !args.is_empty() {
                            self.error(
                                ErrorCode::E0100,
                                format!("channel() takes no arguments, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        return Ty::I64;
                    }
                    // send(ch: i64, value: i64) -> ()
                    if name == "send" {
                        if args.len() != 2 {
                            self.error(
                                ErrorCode::E0513,
                                format!("send() takes exactly 2 arguments, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let ch_ty = self.check_expr(&args[0]);
                        let val_ty = self.check_expr(&args[1]);
                        if !ch_ty.is_error() && !ch_ty.is_integer() {
                            self.error(ErrorCode::E0133, format!("send() first argument must be a channel (integer), found `{ch_ty}`"), args[0].span.clone());
                        }
                        if !val_ty.is_error() && !val_ty.is_integer() {
                            self.error(
                                ErrorCode::E0133,
                                format!("send() second argument must be integer, found `{val_ty}`"),
                                args[1].span.clone(),
                            );
                        }
                        return Ty::Unit;
                    }
                    // recv(ch: i64) -> i64
                    if name == "recv" {
                        if args.len() != 1 {
                            self.error(
                                ErrorCode::E0513,
                                format!("recv() takes exactly 1 argument, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let ch_ty = self.check_expr(&args[0]);
                        if !ch_ty.is_error() && !ch_ty.is_integer() {
                            self.error(
                                ErrorCode::E0133,
                                format!(
                                    "recv() argument must be a channel (integer), found `{ch_ty}`"
                                ),
                                args[0].span.clone(),
                            );
                        }
                        return Ty::I64;
                    }

                    // ── Mutex builtins ────────────────────────────────
                    // mutex(value: i64) -> i64 (mutex pointer)
                    if name == "mutex" {
                        if args.len() != 1 {
                            self.error(
                                ErrorCode::E0100,
                                format!("mutex() takes exactly 1 argument, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let val_ty = self.check_expr(&args[0]);
                        if !val_ty.is_error() && !val_ty.is_integer() {
                            self.error(
                                ErrorCode::E0100,
                                format!("mutex() argument must be integer, found `{val_ty}`"),
                                args[0].span.clone(),
                            );
                        }
                        return Ty::I64;
                    }
                    // mutex_get(m: i64) -> i64
                    if name == "mutex_get" {
                        if args.len() != 1 {
                            self.error(
                                ErrorCode::E0513,
                                format!("mutex_get() takes exactly 1 argument, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let m_ty = self.check_expr(&args[0]);
                        if !m_ty.is_error() && !m_ty.is_integer() {
                            self.error(ErrorCode::E0133, format!("mutex_get() argument must be a mutex (integer), found `{m_ty}`"), args[0].span.clone());
                        }
                        return Ty::I64;
                    }
                    // mutex_set(m: i64, value: i64) -> ()
                    if name == "mutex_set" {
                        if args.len() != 2 {
                            self.error(
                                ErrorCode::E0513,
                                format!(
                                    "mutex_set() takes exactly 2 arguments, got {}",
                                    args.len()
                                ),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let m_ty = self.check_expr(&args[0]);
                        let val_ty = self.check_expr(&args[1]);
                        if !m_ty.is_error() && !m_ty.is_integer() {
                            self.error(ErrorCode::E0133, format!("mutex_set() first argument must be a mutex (integer), found `{m_ty}`"), args[0].span.clone());
                        }
                        if !val_ty.is_error() && !val_ty.is_integer() {
                            self.error(
                                ErrorCode::E0133,
                                format!(
                                    "mutex_set() second argument must be integer, found `{val_ty}`"
                                ),
                                args[1].span.clone(),
                            );
                        }
                        return Ty::Unit;
                    }

                    // clone(val) -> T (requires @derive(Clone))
                    if name == "clone" {
                        if args.len() != 1 {
                            self.error(
                                ErrorCode::E0513,
                                format!("clone() takes exactly 1 argument, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let arg_ty = self.check_expr(&args[0]);
                        if let Ty::Struct(ref struct_name) = arg_ty {
                            if let Some(info) = self.structs.get(struct_name) {
                                if !info.derives.contains(&"Clone".to_string()) {
                                    self.error(ErrorCode::E0100,
                                        format!("cannot clone struct `{struct_name}` without `@derive(Clone)`"),
                                        callee.span.clone(),
                                    );
                                    return Ty::Error;
                                }
                            }
                        } else if !arg_ty.is_error() {
                            self.error(
                                ErrorCode::E0100,
                                format!("clone() expects a struct argument, found `{arg_ty}`"),
                                args[0].span.clone(),
                            );
                            return Ty::Error;
                        }
                        return arg_ty;
                    }

                    // ── HashMap builtins ────────────────────────────────
                    // hashmap() -> i64 (opaque pointer)
                    if name == "hashmap" {
                        if !args.is_empty() {
                            self.error(
                                ErrorCode::E0100,
                                format!("hashmap() takes no arguments, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        return Ty::I64;
                    }
                    // hashmap_set(map: i64, key: str, value: str) -> ()
                    if name == "hashmap_set" {
                        if args.len() != 3 {
                            self.error(
                                ErrorCode::E0513,
                                format!(
                                    "hashmap_set() takes exactly 3 arguments, got {}",
                                    args.len()
                                ),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let map_ty = self.check_expr(&args[0]);
                        let key_ty = self.check_expr(&args[1]);
                        let val_ty = self.check_expr(&args[2]);
                        if !map_ty.is_error() && !map_ty.is_integer() {
                            self.error(ErrorCode::E0133, format!("hashmap_set() first argument must be a hashmap (integer), found `{map_ty}`"), args[0].span.clone());
                        }
                        if !key_ty.is_error() && key_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0133,
                                format!(
                                    "hashmap_set() second argument must be str, found `{key_ty}`"
                                ),
                                args[1].span.clone(),
                            );
                        }
                        if !val_ty.is_error() && val_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0133,
                                format!(
                                    "hashmap_set() third argument must be str, found `{val_ty}`"
                                ),
                                args[2].span.clone(),
                            );
                        }
                        return Ty::Unit;
                    }
                    // hashmap_get(map: i64, key: str) -> str
                    if name == "hashmap_get" {
                        if args.len() != 2 {
                            self.error(
                                ErrorCode::E0513,
                                format!(
                                    "hashmap_get() takes exactly 2 arguments, got {}",
                                    args.len()
                                ),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let map_ty = self.check_expr(&args[0]);
                        let key_ty = self.check_expr(&args[1]);
                        if !map_ty.is_error() && !map_ty.is_integer() {
                            self.error(ErrorCode::E0133, format!("hashmap_get() first argument must be a hashmap (integer), found `{map_ty}`"), args[0].span.clone());
                        }
                        if !key_ty.is_error() && key_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0133,
                                format!(
                                    "hashmap_get() second argument must be str, found `{key_ty}`"
                                ),
                                args[1].span.clone(),
                            );
                        }
                        return Ty::Str;
                    }
                    // hashmap_has(map: i64, key: str) -> bool
                    if name == "hashmap_has" {
                        if args.len() != 2 {
                            self.error(
                                ErrorCode::E0513,
                                format!(
                                    "hashmap_has() takes exactly 2 arguments, got {}",
                                    args.len()
                                ),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let map_ty = self.check_expr(&args[0]);
                        let key_ty = self.check_expr(&args[1]);
                        if !map_ty.is_error() && !map_ty.is_integer() {
                            self.error(ErrorCode::E0133, format!("hashmap_has() first argument must be a hashmap (integer), found `{map_ty}`"), args[0].span.clone());
                        }
                        if !key_ty.is_error() && key_ty != Ty::Str {
                            self.error(
                                ErrorCode::E0133,
                                format!(
                                    "hashmap_has() second argument must be str, found `{key_ty}`"
                                ),
                                args[1].span.clone(),
                            );
                        }
                        return Ty::Bool;
                    }
                    // hashmap_len(map: i64) -> i64
                    if name == "hashmap_len" {
                        if args.len() != 1 {
                            self.error(
                                ErrorCode::E0513,
                                format!(
                                    "hashmap_len() takes exactly 1 argument, got {}",
                                    args.len()
                                ),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let map_ty = self.check_expr(&args[0]);
                        if !map_ty.is_error() && !map_ty.is_integer() {
                            self.error(ErrorCode::E0133, format!("hashmap_len() argument must be a hashmap (integer), found `{map_ty}`"), args[0].span.clone());
                        }
                        return Ty::I64;
                    }
                    // hashmap_keys(map: i64) -> [str]
                    if name == "hashmap_keys" {
                        if args.len() != 1 {
                            self.error(
                                ErrorCode::E0513,
                                format!(
                                    "hashmap_keys() takes exactly 1 argument, got {}",
                                    args.len()
                                ),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let map_ty = self.check_expr(&args[0]);
                        if !map_ty.is_error() && !map_ty.is_integer() {
                            self.error(ErrorCode::E0133, format!("hashmap_keys() argument must be a hashmap (integer), found `{map_ty}`"), args[0].span.clone());
                        }
                        return Ty::Array(Box::new(Ty::Str));
                    }
                    // hashmap_remove(map: i64, key: str) -> ()
                    if name == "hashmap_remove" {
                        if args.len() != 2 {
                            self.error(
                                ErrorCode::E0513,
                                format!(
                                    "hashmap_remove() takes exactly 2 arguments, got {}",
                                    args.len()
                                ),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let map_ty = self.check_expr(&args[0]);
                        let key_ty = self.check_expr(&args[1]);
                        if !map_ty.is_error() && !map_ty.is_integer() {
                            self.error(ErrorCode::E0133, format!("hashmap_remove() first argument must be a hashmap (integer), found `{map_ty}`"), args[0].span.clone());
                        }
                        if !key_ty.is_error() && key_ty != Ty::Str {
                            self.error(ErrorCode::E0133, format!("hashmap_remove() second argument must be str, found `{key_ty}`"), args[1].span.clone());
                        }
                        return Ty::Unit;
                    }

                    // ── Unsafe builtins ────────────────────────────────
                    // deref(addr: i64) -> i64 — raw memory load (unsafe only)
                    if name == "deref" {
                        if !self.in_unsafe_context {
                            self.error(
                                ErrorCode::E0100,
                                "`deref()` can only be called inside an `@unsafe` function"
                                    .to_string(),
                                callee.span.clone(),
                            );
                        }
                        if args.len() != 1 {
                            self.error(
                                ErrorCode::E0100,
                                format!("deref() takes exactly 1 argument, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let addr_ty = self.check_expr(&args[0]);
                        if !addr_ty.is_error() && !addr_ty.is_integer() {
                            self.error(
                                ErrorCode::E0100,
                                format!("deref() argument must be i64, found `{addr_ty}`"),
                                args[0].span.clone(),
                            );
                        }
                        return Ty::I64;
                    }
                    // store(addr: i64, value: i64) — raw memory store (unsafe only)
                    if name == "store" {
                        if !self.in_unsafe_context {
                            self.error(
                                ErrorCode::E0100,
                                "`store()` can only be called inside an `@unsafe` function"
                                    .to_string(),
                                callee.span.clone(),
                            );
                        }
                        if args.len() != 2 {
                            self.error(
                                ErrorCode::E0100,
                                format!("store() takes exactly 2 arguments, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let addr_ty = self.check_expr(&args[0]);
                        let val_ty = self.check_expr(&args[1]);
                        if !addr_ty.is_error() && !addr_ty.is_integer() {
                            self.error(
                                ErrorCode::E0100,
                                format!("store() first argument must be i64, found `{addr_ty}`"),
                                args[0].span.clone(),
                            );
                        }
                        if !val_ty.is_error() && !val_ty.is_integer() {
                            self.error(
                                ErrorCode::E0100,
                                format!("store() second argument must be i64, found `{val_ty}`"),
                                args[1].span.clone(),
                            );
                        }
                        return Ty::Unit;
                    }

                    // map(arr, fn) -> [U]
                    if name == "map" {
                        if args.len() != 2 {
                            self.error(
                                ErrorCode::E0513,
                                format!("map() takes exactly 2 arguments, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let arr_ty = self.check_expr(&args[0]);
                        if let Ty::Array(ref inner) = arr_ty {
                            self.closure_param_hint = Some(vec![*inner.clone()]);
                        }
                        let fn_ty = self.check_expr(&args[1]);
                        let elem_ty = match &arr_ty {
                            Ty::Array(inner) => *inner.clone(),
                            _ if arr_ty.is_error() => Ty::Error,
                            _ => {
                                self.error(
                                    ErrorCode::E0133,
                                    format!(
                                        "map() first argument must be an array, found `{arr_ty}`"
                                    ),
                                    args[0].span.clone(),
                                );
                                return Ty::Error;
                            }
                        };
                        match &fn_ty {
                            Ty::Fn(params, ret) => {
                                if params.len() != 1 {
                                    self.error(
                                        ErrorCode::E0133,
                                        format!(
                                            "map() callback must take 1 parameter, takes {}",
                                            params.len()
                                        ),
                                        args[1].span.clone(),
                                    );
                                } else if !elem_ty.is_error()
                                    && !params[0].is_error()
                                    && elem_ty != params[0]
                                {
                                    self.error(ErrorCode::E0100,
                                        format!("map() callback parameter type `{}` doesn't match array element type `{}`", params[0], elem_ty),
                                        args[1].span.clone(),
                                    );
                                }
                                return Ty::Array(ret.clone());
                            }
                            _ if fn_ty.is_error() => return Ty::Error,
                            _ => {
                                self.error(
                                    ErrorCode::E0133,
                                    format!(
                                        "map() second argument must be a function, found `{fn_ty}`"
                                    ),
                                    args[1].span.clone(),
                                );
                                return Ty::Error;
                            }
                        }
                    }

                    // filter(arr, fn) -> [T]
                    if name == "filter" {
                        if args.len() != 2 {
                            self.error(
                                ErrorCode::E0513,
                                format!("filter() takes exactly 2 arguments, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let arr_ty = self.check_expr(&args[0]);
                        if let Ty::Array(ref inner) = arr_ty {
                            self.closure_param_hint = Some(vec![*inner.clone()]);
                        }
                        let fn_ty = self.check_expr(&args[1]);
                        let elem_ty = match &arr_ty {
                            Ty::Array(inner) => *inner.clone(),
                            _ if arr_ty.is_error() => Ty::Error,
                            _ => {
                                self.error(ErrorCode::E0133,
                                    format!("filter() first argument must be an array, found `{arr_ty}`"),
                                    args[0].span.clone(),
                                );
                                return Ty::Error;
                            }
                        };
                        match &fn_ty {
                            Ty::Fn(params, ret) => {
                                if params.len() != 1 {
                                    self.error(
                                        ErrorCode::E0133,
                                        format!(
                                            "filter() callback must take 1 parameter, takes {}",
                                            params.len()
                                        ),
                                        args[1].span.clone(),
                                    );
                                } else if !elem_ty.is_error()
                                    && !params[0].is_error()
                                    && elem_ty != params[0]
                                {
                                    self.error(ErrorCode::E0100,
                                        format!("filter() callback parameter type `{}` doesn't match array element type `{}`", params[0], elem_ty),
                                        args[1].span.clone(),
                                    );
                                }
                                if **ret != Ty::Bool && !ret.is_error() {
                                    self.error(
                                        ErrorCode::E0133,
                                        format!(
                                            "filter() callback must return `bool`, returns `{}`",
                                            ret
                                        ),
                                        args[1].span.clone(),
                                    );
                                }
                                return arr_ty;
                            }
                            _ if fn_ty.is_error() => return Ty::Error,
                            _ => {
                                self.error(ErrorCode::E0133,
                                    format!("filter() second argument must be a function, found `{fn_ty}`"),
                                    args[1].span.clone(),
                                );
                                return Ty::Error;
                            }
                        }
                    }

                    // reduce(arr, init, fn) -> U
                    if name == "reduce" {
                        if args.len() != 3 {
                            self.error(
                                ErrorCode::E0513,
                                format!("reduce() takes exactly 3 arguments, got {}", args.len()),
                                callee.span.clone(),
                            );
                            return Ty::Error;
                        }
                        let arr_ty = self.check_expr(&args[0]);
                        let init_ty = self.check_expr(&args[1]);
                        {
                            let elem = match &arr_ty {
                                Ty::Array(inner) => *inner.clone(),
                                _ => Ty::Error,
                            };
                            self.closure_param_hint = Some(vec![init_ty.clone(), elem]);
                        }
                        let fn_ty = self.check_expr(&args[2]);
                        let elem_ty = match &arr_ty {
                            Ty::Array(inner) => *inner.clone(),
                            _ if arr_ty.is_error() => Ty::Error,
                            _ => {
                                self.error(ErrorCode::E0133,
                                    format!("reduce() first argument must be an array, found `{arr_ty}`"),
                                    args[0].span.clone(),
                                );
                                return Ty::Error;
                            }
                        };
                        match &fn_ty {
                            Ty::Fn(params, ret) => {
                                if params.len() != 2 {
                                    self.error(
                                        ErrorCode::E0133,
                                        format!(
                                            "reduce() callback must take 2 parameters, takes {}",
                                            params.len()
                                        ),
                                        args[2].span.clone(),
                                    );
                                } else {
                                    if !init_ty.is_error()
                                        && !params[0].is_error()
                                        && init_ty != params[0]
                                    {
                                        self.error(ErrorCode::E0133,
                                            format!("reduce() callback first parameter type `{}` doesn't match initial value type `{}`", params[0], init_ty),
                                            args[2].span.clone(),
                                        );
                                    }
                                    if !elem_ty.is_error()
                                        && !params[1].is_error()
                                        && elem_ty != params[1]
                                    {
                                        self.error(ErrorCode::E0133,
                                            format!("reduce() callback second parameter type `{}` doesn't match array element type `{}`", params[1], elem_ty),
                                            args[2].span.clone(),
                                        );
                                    }
                                }
                                return *ret.clone();
                            }
                            _ if fn_ty.is_error() => return Ty::Error,
                            _ => {
                                self.error(ErrorCode::E0133,
                                    format!("reduce() third argument must be a function, found `{fn_ty}`"),
                                    args[2].span.clone(),
                                );
                                return Ty::Error;
                            }
                        }
                    }

                    // Check if callee is a variable with fn type (closure call)
                    if let Some(info) = self.lookup_var(name).cloned() {
                        if let Ty::Fn(ref param_tys, ref ret_ty) = info.ty {
                            if args.len() != param_tys.len() {
                                self.error(
                                    ErrorCode::E0100,
                                    format!(
                                        "closure expects {} argument(s) but {} were given",
                                        param_tys.len(),
                                        args.len()
                                    ),
                                    callee.span.clone(),
                                );
                                return *ret_ty.clone();
                            }
                            for (i, arg) in args.iter().enumerate() {
                                let arg_ty = self.check_expr(arg);
                                if !arg_ty.contains_error()
                                    && !param_tys[i].contains_error()
                                    && !types_compatible(&param_tys[i], &arg_ty)
                                    && arg_ty != param_tys[i]
                                {
                                    self.error(
                                        ErrorCode::E0100,
                                        format!(
                                            "argument {} expects `{}`, found `{arg_ty}`",
                                            i + 1,
                                            param_tys[i]
                                        ),
                                        arg.span.clone(),
                                    );
                                }
                            }
                            return *ret_ty.clone();
                        }
                        // Variable exists but is not a function type -- fall through to check named functions
                    }

                    // User-defined function
                    if let Some(sig) = self.functions.get(name).cloned() {
                        // Calling an @unsafe function from a safe context is an error
                        if sig.is_unsafe && !self.in_unsafe_context {
                            self.error(
                                ErrorCode::E0100,
                                format!(
                                    "cannot call `@unsafe` function `{name}` from a safe context"
                                ),
                                callee.span.clone(),
                            );
                        }
                        if args.len() != sig.params.len() {
                            self.error(
                                ErrorCode::E0100,
                                format!(
                                    "function `{name}` expects {} argument(s) but {} were given",
                                    sig.params.len(),
                                    args.len()
                                ),
                                callee.span.clone(),
                            );
                            return self.substitute_return_type(&sig, &HashMap::new());
                        }

                        // Check arguments and build substitution map for generic type params
                        let mut substitutions: HashMap<String, Ty> = HashMap::new();
                        let mut arg_types = Vec::new();
                        for (i, arg) in args.iter().enumerate() {
                            let arg_ty = self.check_expr(arg);
                            arg_types.push(arg_ty.clone());
                            let (_, ref param_ty) = &sig.params[i];

                            // If param type is a type parameter, infer its concrete type
                            if let Ty::TypeParam(ref tp_name) = param_ty {
                                if let Some(existing) = substitutions.get(tp_name) {
                                    // T already inferred -- check consistency
                                    if !arg_ty.is_error()
                                        && !existing.is_error()
                                        && arg_ty != *existing
                                    {
                                        self.error(ErrorCode::E0100,
                                            format!(
                                                "type parameter `{tp_name}` inferred as `{existing}` but argument has type `{arg_ty}`"
                                            ),
                                            arg.span.clone(),
                                        );
                                    }
                                } else if !arg_ty.is_error() {
                                    substitutions.insert(tp_name.clone(), arg_ty.clone());
                                }
                            }
                        }

                        // Now check argument types against substituted parameter types
                        for (i, arg) in args.iter().enumerate() {
                            let arg_ty = &arg_types[i];
                            let (ref param_name, ref param_ty) = &sig.params[i];
                            let concrete_param_ty = self.substitute_ty(param_ty, &substitutions);
                            if !arg_ty.contains_error()
                                && !concrete_param_ty.contains_error()
                                && !matches!(concrete_param_ty, Ty::TypeParam(_))
                                && !types_compatible(&concrete_param_ty, arg_ty)
                                && *arg_ty != concrete_param_ty
                            {
                                // Allow integer literal coercion: i64 literal -> i32, u32, u64
                                let is_int_literal_coercion = *arg_ty == Ty::I64
                                    && matches!(concrete_param_ty, Ty::I32 | Ty::U32 | Ty::U64)
                                    && matches!(&arg.node, Expr::IntLit(n) if
                                        match concrete_param_ty {
                                            Ty::U32 | Ty::U64 => *n >= 0,
                                            _ => true,
                                        }
                                    );
                                if !is_int_literal_coercion {
                                    self.error(ErrorCode::E0100,
                                        format!(
                                            "argument `{param_name}` expects `{concrete_param_ty}`, found `{arg_ty}`"
                                        ),
                                        arg.span.clone(),
                                    );
                                }
                            }
                        }

                        // Check trait bounds for each inferred type parameter
                        for (tp_name, concrete_ty) in &substitutions {
                            if let Some(bounds) = sig.type_param_bounds.get(tp_name) {
                                for bound in bounds {
                                    let type_name = match concrete_ty {
                                        Ty::Struct(s) => Some(s.as_str()),
                                        _ => None,
                                    };
                                    let has_impl = type_name.is_some_and(|tn| {
                                        self.trait_impls
                                            .get(tn)
                                            .is_some_and(|impls| impls.contains(bound))
                                    });
                                    if !has_impl && !concrete_ty.is_error() {
                                        self.error(ErrorCode::E0100,
                                            format!(
                                                "type `{concrete_ty}` does not implement trait `{bound}`"
                                            ),
                                            callee.span.clone(),
                                        );
                                    }
                                }
                            }
                        }

                        self.substitute_return_type(&sig, &substitutions)
                    } else {
                        // Check if this is an enum variant construction via UFCS rewrite:
                        // Parser transforms Shape.Circle(5.0) into Call { callee: Ident("Circle"), args: [Ident("Shape"), 5.0] }
                        if !args.is_empty() {
                            if let Expr::Ident(ref first_name) = args[0].node {
                                if let Some(info) = self.enums.get(first_name).cloned() {
                                    if let Some(field_tys) = info.variant_fields(name) {
                                        // This is an enum variant construction
                                        let expected_args = field_tys.len();
                                        let actual_args = args.len() - 1; // subtract the enum type name
                                        if actual_args != expected_args {
                                            self.error(ErrorCode::E0100,
                                                format!(
                                                    "variant `{name}` of enum `{first_name}` expects {} argument(s) but {} were given",
                                                    expected_args, actual_args
                                                ),
                                                callee.span.clone(),
                                            );
                                        }
                                        // Type-check arguments against variant field types
                                        for (i, arg) in args.iter().skip(1).enumerate() {
                                            let arg_ty = self.check_expr(arg);
                                            // Skip type check for generic (TypeParam) fields
                                            if i < field_tys.len()
                                                && !matches!(&field_tys[i], Ty::TypeParam(_))
                                                && !arg_ty.is_error()
                                                && !field_tys[i].is_error()
                                                && arg_ty != field_tys[i]
                                            {
                                                self.error(ErrorCode::E0100,
                                                    format!(
                                                        "variant `{name}` field {} expects `{}`, found `{arg_ty}`",
                                                        i + 1, field_tys[i]
                                                    ),
                                                    arg.span.clone(),
                                                );
                                            }
                                        }
                                        return Ty::Enum(first_name.clone());
                                    }
                                }
                            }
                        }

                        // Before reporting "undefined function", check if this is a UFCS method call.
                        // The parser transforms `obj.method(args)` into `method(obj, args)`,
                        // so the first arg is the receiver.
                        if !args.is_empty() {
                            let first_arg_ty = self.check_expr(&args[0]);
                            if let Ty::Struct(ref type_name) = first_arg_ty {
                                if let Some(method_sig) = self
                                    .methods
                                    .get(type_name)
                                    .and_then(|m| m.get(name))
                                    .cloned()
                                {
                                    // Check argument count (all args including self)
                                    if args.len() != method_sig.params.len() {
                                        self.error(ErrorCode::E0100,
                                            format!(
                                                "method `{name}` on `{type_name}` expects {} argument(s) but {} were given",
                                                method_sig.params.len() - 1,
                                                args.len() - 1
                                            ),
                                            callee.span.clone(),
                                        );
                                        return method_sig.ret;
                                    }
                                    // Check argument types (skip self at index 0, already checked)
                                    for (i, arg) in args.iter().skip(1).enumerate() {
                                        let arg_ty = self.check_expr(arg);
                                        let (ref param_name, ref param_ty) =
                                            method_sig.params[i + 1];
                                        if !arg_ty.contains_error()
                                            && !param_ty.contains_error()
                                            && !types_compatible(param_ty, &arg_ty)
                                            && arg_ty != *param_ty
                                        {
                                            self.error(ErrorCode::E0100,
                                                format!("argument `{param_name}` expects `{param_ty}`, found `{arg_ty}`"),
                                                arg.span.clone(),
                                            );
                                        }
                                    }
                                    return method_sig.ret;
                                }
                            }
                        }
                        self.error(
                            ErrorCode::E0301,
                            format!("undefined function `{name}`"),
                            callee.span.clone(),
                        );
                        Ty::Error
                    }
                } else if let Expr::FieldAccess { object, field } = &callee.node {
                    // Method call: object.method(args) — fallback for non-UFCS path
                    let obj_ty = self.check_expr(object);
                    if let Ty::Struct(ref type_name) = obj_ty {
                        if let Some(method_sig) = self
                            .methods
                            .get(type_name)
                            .and_then(|m| m.get(field))
                            .cloned()
                        {
                            let expected_args = method_sig.params.len() - 1;
                            if args.len() != expected_args {
                                self.error(ErrorCode::E0100,
                                    format!(
                                        "method `{field}` on `{type_name}` expects {} argument(s) but {} were given",
                                        expected_args, args.len()
                                    ),
                                    callee.span.clone(),
                                );
                                return method_sig.ret;
                            }
                            for (i, arg) in args.iter().enumerate() {
                                let arg_ty = self.check_expr(arg);
                                let (ref param_name, ref param_ty) = method_sig.params[i + 1];
                                if !arg_ty.contains_error() && !param_ty.contains_error()
                                    && !types_compatible(param_ty, &arg_ty)
                                    && arg_ty != *param_ty
                                {
                                    self.error(ErrorCode::E0100,
                                        format!("argument `{param_name}` expects `{param_ty}`, found `{arg_ty}`"),
                                        arg.span.clone(),
                                    );
                                }
                            }
                            method_sig.ret
                        } else {
                            self.error(
                                ErrorCode::E0317,
                                format!("no method `{field}` found on type `{type_name}`"),
                                callee.span.clone(),
                            );
                            Ty::Error
                        }
                    } else if obj_ty.is_error() {
                        Ty::Error
                    } else {
                        self.error(
                            ErrorCode::E0134,
                            format!("cannot call method `{field}` on type `{obj_ty}`"),
                            callee.span.clone(),
                        );
                        Ty::Error
                    }
                } else {
                    self.error(
                        ErrorCode::E0512,
                        "only named function calls are supported".to_string(),
                        callee.span.clone(),
                    );
                    Ty::Error
                }
            }

            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond_ty = self.check_expr(condition);
                if !cond_ty.is_error() && cond_ty != Ty::Bool {
                    // Allow integer conditions (truthy)
                    if !cond_ty.is_integer() {
                        self.error(
                            ErrorCode::E0116,
                            format!("if condition must be `bool`, found `{cond_ty}`"),
                            condition.span.clone(),
                        );
                    }
                }

                let then_ty = self.check_expr(then_branch);

                if let Some(else_expr) = else_branch {
                    let else_ty = self.check_expr(else_expr);
                    // If used as expression (both branches must match)
                    if !then_ty.is_error()
                        && !else_ty.is_error()
                        && !types_compatible(&then_ty, &else_ty)
                    {
                        // Only warn if both are non-unit (meaning it's used as an expression)
                        if then_ty != Ty::Unit && else_ty != Ty::Unit {
                            self.error(ErrorCode::E0107,
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
                            ErrorCode::E0501,
                            format!("cannot assign to immutable variable `{target}`"),
                            expr.span.clone(),
                        );
                    }
                    if !val_ty.contains_error() && !info.ty.contains_error()
                        && !types_compatible(&info.ty, &val_ty) && val_ty != info.ty
                    {
                        self.error(
                            ErrorCode::E0111,
                            format!(
                                "cannot assign `{val_ty}` to variable `{target}` of type `{}`",
                                info.ty
                            ),
                            value.span.clone(),
                        );
                    }
                } else {
                    self.error(
                        ErrorCode::E0300,
                        format!("undefined variable `{target}`"),
                        expr.span.clone(),
                    );
                }

                Ty::Unit
            }

            Expr::CompoundAssign { target, op, value } => {
                let val_ty = self.check_expr(value);

                let op_str = match op {
                    BinOp::Add => "+",
                    BinOp::Sub => "-",
                    BinOp::Mul => "*",
                    BinOp::Div => "/",
                    BinOp::Mod => "%",
                    _ => {
                        self.error(
                            ErrorCode::E0137,
                            "unsupported compound assignment operator".to_string(),
                            expr.span.clone(),
                        );
                        return Ty::Unit;
                    }
                };

                if let Some(info) = self.lookup_var(target).cloned() {
                    if !info.mutable {
                        self.error(
                            ErrorCode::E0501,
                            format!("cannot assign to immutable variable `{target}`"),
                            expr.span.clone(),
                        );
                    }
                    if !val_ty.contains_error() && !info.ty.contains_error()
                        && !types_compatible(&info.ty, &val_ty) && val_ty != info.ty
                    {
                        self.error(ErrorCode::E0130,
                            format!(
                                "cannot apply `{op_str}=` with `{val_ty}` to variable `{target}` of type `{}`",
                                info.ty
                            ),
                            value.span.clone(),
                        );
                    }
                    if !info.ty.is_numeric() && !info.ty.is_error() {
                        self.error(
                            ErrorCode::E0300,
                            format!("cannot perform arithmetic on `{}`", info.ty),
                            expr.span.clone(),
                        );
                    }
                } else {
                    self.error(
                        ErrorCode::E0300,
                        format!("undefined variable `{target}`"),
                        expr.span.clone(),
                    );
                }

                Ty::Unit
            }

            Expr::FieldAssign {
                object,
                field,
                value,
            } => {
                let val_ty = self.check_expr(value);
                let obj_ty = self.check_expr(object);

                // Check mutability of the root variable
                if let Some(root_name) = Self::root_var_name(object) {
                    if let Some(info) = self.lookup_var(&root_name) {
                        if !info.mutable {
                            self.error(ErrorCode::E0502,
                                format!("cannot assign to field `{field}` of immutable variable `{root_name}` (declare with `let mut` to make mutable)"),
                                expr.span.clone(),
                            );
                        }
                    }
                }

                // Check field exists and type matches
                if let Ty::Struct(struct_name) = &obj_ty {
                    if let Some(struct_info) = self.structs.get(struct_name).cloned() {
                        if let Some((_, field_ty)) =
                            struct_info.fields.iter().find(|(n, _)| n == field)
                        {
                            // Skip type check for generic (TypeParam) fields — they accept any type
                            if !matches!(field_ty, Ty::TypeParam(_))
                                && !val_ty.is_error()
                                && !field_ty.is_error()
                                && val_ty != *field_ty
                            {
                                self.error(ErrorCode::E0112,
                                    format!(
                                        "cannot assign `{val_ty}` to field `{field}` of type `{field_ty}`"
                                    ),
                                    value.span.clone(),
                                );
                            }
                        } else {
                            self.error(
                                ErrorCode::E0315,
                                format!("struct `{struct_name}` has no field `{field}`"),
                                expr.span.clone(),
                            );
                        }
                    }
                } else if !obj_ty.is_error() {
                    self.error(
                        ErrorCode::E0135,
                        format!("cannot assign to field `{field}` on non-struct type `{obj_ty}`"),
                        object.span.clone(),
                    );
                }

                Ty::Unit
            }

            Expr::IndexAssign {
                object,
                index,
                value,
            } => {
                let val_ty = self.check_expr(value);
                let obj_ty = self.check_expr(object);
                let idx_ty = self.check_expr(index);

                // Check mutability of the root variable
                if let Some(root_name) = Self::root_var_name(object) {
                    if let Some(info) = self.lookup_var(&root_name) {
                        if !info.mutable {
                            self.error(ErrorCode::E0503,
                                format!("cannot assign to index of immutable variable `{root_name}` (declare with `let mut` to make mutable)"),
                                expr.span.clone(),
                            );
                        }
                    }
                }

                // Check index is integer
                if !idx_ty.is_error() && !idx_ty.is_integer() {
                    self.error(
                        ErrorCode::E0123,
                        format!("array index must be an integer, found `{idx_ty}`"),
                        index.span.clone(),
                    );
                }

                // Check object is array and value type matches element type
                match &obj_ty {
                    Ty::Array(inner) => {
                        if !val_ty.is_error() && !inner.is_error() && val_ty != **inner {
                            self.error(
                                ErrorCode::E0113,
                                format!("cannot assign `{val_ty}` to array of `{inner}`"),
                                value.span.clone(),
                            );
                        }
                    }
                    Ty::Error => {}
                    _ => {
                        self.error(
                            ErrorCode::E0124,
                            format!("cannot index-assign into `{obj_ty}`"),
                            object.span.clone(),
                        );
                    }
                }

                Ty::Unit
            }

            Expr::While { condition, body } => {
                let cond_ty = self.check_expr(condition);
                if !cond_ty.is_error() && cond_ty != Ty::Bool && !cond_ty.is_integer() {
                    self.error(
                        ErrorCode::E0117,
                        format!("while condition must be `bool`, found `{cond_ty}`"),
                        condition.span.clone(),
                    );
                }
                self.loop_depth += 1;
                self.check_expr(body);
                self.loop_depth -= 1;
                Ty::Unit
            }

            Expr::Await(inner) => {
                let ty = self.check_expr(inner);
                // In Sprint 9, await on a Future<T> yields T.
                // Await on a non-future type just passes through (sync await).
                match ty {
                    Ty::Future(inner_ty) => *inner_ty,
                    other => other,
                }
            }

            Expr::Spawn(inner) => {
                let ty = self.check_expr(inner);
                // spawn wraps the result in Future<T>
                if ty.is_error() {
                    Ty::Error
                } else {
                    Ty::Future(Box::new(ty))
                }
            }

            Expr::Try(inner) => {
                let inner_ty = self.check_expr(inner);
                if inner_ty.is_error() {
                    return Ty::Error;
                }
                match &inner_ty {
                    Ty::Result(ok_ty, _err_ty) => {
                        // The enclosing function must also return a Result type
                        match &self.current_return_type {
                            Ty::Result(_, _) => {}
                            _ => {
                                self.error(ErrorCode::E0121,
                                    "`?` operator can only be used in a function that returns a Result type".to_string(),
                                    expr.span.clone(),
                                );
                            }
                        }
                        *ok_ty.clone()
                    }
                    _ => {
                        self.error(
                            ErrorCode::E0120,
                            format!("`?` operator requires a Result type, found `{inner_ty}`"),
                            inner.span.clone(),
                        );
                        Ty::Error
                    }
                }
            }

            Expr::Range { start, end } => {
                let start_ty = self.check_expr(start);
                let end_ty = self.check_expr(end);
                if !start_ty.is_error() && !start_ty.is_integer() {
                    self.error(
                        ErrorCode::E0122,
                        format!("range start must be an integer, found `{start_ty}`"),
                        start.span.clone(),
                    );
                }
                if !end_ty.is_error() && !end_ty.is_integer() {
                    self.error(
                        ErrorCode::E0122,
                        format!("range end must be an integer, found `{end_ty}`"),
                        end.span.clone(),
                    );
                }
                Ty::Unit // Range type (treated as unit for now)
            }

            Expr::ForIn {
                var_name,
                iterable,
                body,
            } => {
                // Check the iterable expression
                let iter_ty = self.check_expr(iterable);

                // Infer element type from the iterable
                let elem_ty = match &iter_ty {
                    Ty::Array(inner) => *inner.clone(),
                    _ if !iter_ty.is_error() => {
                        // Range or unknown -- default to I64 for ranges
                        Ty::I64
                    }
                    _ => Ty::Error,
                };

                self.push_scope();
                self.define_var(
                    var_name,
                    VarInfo {
                        ty: elem_ty,
                        mutable: false,
                    },
                    &expr.span,
                );
                self.loop_depth += 1;
                self.check_expr(body);
                self.loop_depth -= 1;
                self.pop_scope();
                Ty::Unit
            }

            Expr::ArrayLit(elements) => {
                if elements.is_empty() {
                    self.error(
                        ErrorCode::E0115,
                        "cannot infer type of empty array".to_string(),
                        expr.span.clone(),
                    );
                    return Ty::Error;
                }
                let first_ty = self.check_expr(&elements[0]);
                for elem in &elements[1..] {
                    let elem_ty = self.check_expr(elem);
                    if !elem_ty.is_error() && !first_ty.is_error() && elem_ty != first_ty {
                        self.error(ErrorCode::E0114,
                            format!("array elements must all have the same type, expected `{first_ty}` but found `{elem_ty}`"),
                            elem.span.clone(),
                        );
                    }
                }
                Ty::Array(Box::new(first_ty))
            }

            Expr::Index { object, index } => {
                let obj_ty = self.check_expr(object);
                let idx_ty = self.check_expr(index);
                if !idx_ty.is_error() && !idx_ty.is_integer() {
                    self.error(
                        ErrorCode::E0123,
                        format!("array index must be an integer, found `{idx_ty}`"),
                        index.span.clone(),
                    );
                }
                match &obj_ty {
                    Ty::Array(inner) => *inner.clone(),
                    Ty::Error => Ty::Error,
                    _ => {
                        self.error(
                            ErrorCode::E0124,
                            format!("cannot index into `{obj_ty}`"),
                            object.span.clone(),
                        );
                        Ty::Error
                    }
                }
            }

            Expr::StructLit { name, fields } => {
                // Check if this is an agent instantiation: AgentName {}
                if self.agents.contains_key(name) {
                    if !fields.is_empty() {
                        self.error(ErrorCode::E0511,
                            format!("agent `{name}` does not accept field initializers; use `{name} {{}}` to instantiate"),
                            expr.span.clone(),
                        );
                    }
                    return Ty::Agent(name.clone());
                }

                let Some(struct_info) = self.structs.get(name).cloned() else {
                    self.error(
                        ErrorCode::E0302,
                        format!("undefined struct `{name}`"),
                        expr.span.clone(),
                    );
                    // Still check field value expressions
                    for (_, value) in fields {
                        self.check_expr(value);
                    }
                    return Ty::Error;
                };

                // Check that all fields are provided and types match
                let expected_fields: HashMap<&str, &Ty> = struct_info
                    .fields
                    .iter()
                    .map(|(n, t)| (n.as_str(), t))
                    .collect();

                // Track type parameter inference for generic structs
                let mut tp_inferred: HashMap<String, Ty> = HashMap::new();

                let mut provided = std::collections::HashSet::new();
                for (field_name, value) in fields {
                    let val_ty = self.check_expr(value);
                    if let Some(expected_ty) = expected_fields.get(field_name.as_str()) {
                        if let Ty::TypeParam(ref tp_name) = expected_ty {
                            // Generic field: infer or check consistency
                            if !val_ty.is_error() {
                                if let Some(prev) = tp_inferred.get(tp_name) {
                                    if prev != &val_ty {
                                        self.error(ErrorCode::E0131,
                                            format!(
                                                "type parameter `{tp_name}` in struct `{name}` inferred as `{prev}` but field `{field_name}` has type `{val_ty}`"
                                            ),
                                            value.span.clone(),
                                        );
                                    }
                                } else {
                                    tp_inferred.insert(tp_name.clone(), val_ty.clone());
                                }
                            }
                        } else if !val_ty.is_error()
                            && !expected_ty.is_error()
                            && &val_ty != *expected_ty
                        {
                            self.error(ErrorCode::E0100,
                                format!(
                                    "field `{field_name}` of struct `{name}` expects `{}`, found `{val_ty}`",
                                    expected_ty
                                ),
                                value.span.clone(),
                            );
                        }
                        provided.insert(field_name.as_str());
                    } else {
                        self.error(
                            ErrorCode::E0315,
                            format!("struct `{name}` has no field `{field_name}`"),
                            value.span.clone(),
                        );
                    }
                }

                // Check for missing fields
                for (field_name, _) in &struct_info.fields {
                    if !provided.contains(field_name.as_str()) {
                        self.error(
                            ErrorCode::E0318,
                            format!("missing field `{field_name}` in struct `{name}`"),
                            expr.span.clone(),
                        );
                    }
                }

                Ty::Struct(name.clone())
            }

            Expr::FieldAccess { object, field } => {
                // Check if this is actually an enum variant access: EnumName.VariantName
                if let Expr::Ident(ref name) = object.node {
                    if let Some(info) = self.enums.get(name).cloned() {
                        if !info.has_variant(field) {
                            self.error(
                                ErrorCode::E0316,
                                format!("enum `{name}` has no variant `{field}`"),
                                expr.span.clone(),
                            );
                        } else if let Some(fields) = info.variant_fields(field) {
                            if !fields.is_empty() {
                                self.error(ErrorCode::E0100,
                                    format!("variant `{field}` of enum `{name}` requires {} argument(s)", fields.len()),
                                    expr.span.clone(),
                                );
                            }
                        }
                        return Ty::Enum(name.clone());
                    }
                }

                let obj_ty = self.check_expr(object);
                match &obj_ty {
                    Ty::Struct(struct_name) => {
                        if let Some(struct_info) = self.structs.get(struct_name).cloned() {
                            if let Some((_, field_ty)) =
                                struct_info.fields.iter().find(|(n, _)| n == field)
                            {
                                field_ty.clone()
                            } else {
                                self.error(
                                    ErrorCode::E0315,
                                    format!("struct `{struct_name}` has no field `{field}`"),
                                    expr.span.clone(),
                                );
                                Ty::Error
                            }
                        } else {
                            self.error(
                                ErrorCode::E0302,
                                format!("undefined struct `{struct_name}`"),
                                expr.span.clone(),
                            );
                            Ty::Error
                        }
                    }
                    Ty::Agent(agent_name) => match field.as_str() {
                        "model" => Ty::Str,
                        "system" => Ty::Str,
                        "tools" => Ty::Array(Box::new(Ty::Str)),
                        _ => {
                            self.error(ErrorCode::E0315,
                                    format!("agent `{agent_name}` has no field `{field}`; available fields: model, system, tools"),
                                    expr.span.clone(),
                                );
                            Ty::Error
                        }
                    },
                    Ty::Error => Ty::Error,
                    _ => {
                        self.error(
                            ErrorCode::E0135,
                            format!("cannot access field `{field}` on type `{obj_ty}`"),
                            object.span.clone(),
                        );
                        Ty::Error
                    }
                }
            }

            Expr::EnumVariant { enum_name, variant } => {
                if let Some(info) = self.enums.get(enum_name) {
                    if !info.has_variant(variant) {
                        self.error(
                            ErrorCode::E0316,
                            format!("enum `{enum_name}` has no variant `{variant}`"),
                            expr.span.clone(),
                        );
                    }
                    Ty::Enum(enum_name.clone())
                } else {
                    self.error(
                        ErrorCode::E0303,
                        format!("undefined enum `{enum_name}`"),
                        expr.span.clone(),
                    );
                    Ty::Error
                }
            }

            Expr::Match { subject, arms } => {
                let subject_ty = self.check_expr(subject);

                if arms.is_empty() {
                    self.error(
                        ErrorCode::E0201,
                        "match expression has no arms".to_string(),
                        expr.span.clone(),
                    );
                    return Ty::Error;
                }

                // Check each arm's pattern and body
                let mut result_ty: Option<Ty> = None;
                let mut has_wildcard = false;
                let mut covered_variants: Vec<String> = Vec::new();

                for arm in arms {
                    // Validate pattern against subject type
                    self.check_pattern(&arm.pattern, &subject_ty);

                    // Track coverage for exhaustiveness
                    match &arm.pattern.node {
                        Pattern::Wildcard => {
                            has_wildcard = true;
                        }
                        Pattern::Ident(name) => {
                            // For enums, ident patterns are variant names
                            if let Ty::Enum(_) = &subject_ty {
                                covered_variants.push(name.clone());
                            } else {
                                // For non-enum types, an ident pattern is a
                                // variable binding which acts as a wildcard
                                has_wildcard = true;
                            }
                        }
                        Pattern::BoolLit(b) => {
                            covered_variants.push(b.to_string());
                        }
                        Pattern::Ok(_) => {
                            covered_variants.push("ok".to_string());
                        }
                        Pattern::Err(_) => {
                            covered_variants.push("err".to_string());
                        }
                        Pattern::Some(_) => {
                            covered_variants.push("some".to_string());
                        }
                        Pattern::None => {
                            covered_variants.push("none".to_string());
                        }
                        Pattern::VariantDestructure { variant, .. } => {
                            covered_variants.push(variant.clone());
                        }
                        _ => {} // IntLit and StringLit don't cover the full domain
                    }

                    // For patterns with bindings, push a scope so both guards and bodies
                    // can reference destructured variables.
                    let body_ty = match &arm.pattern.node {
                        Pattern::Ok(binding) => {
                            self.push_scope();
                            let ok_ty = if let Ty::Result(ref ok, _) = subject_ty {
                                *ok.clone()
                            } else {
                                Ty::Error
                            };
                            self.define_var(
                                binding,
                                VarInfo {
                                    ty: ok_ty,
                                    mutable: false,
                                },
                                &arm.pattern.span,
                            );
                            if let Some(ref guard) = arm.guard {
                                let guard_ty = self.check_expr(guard);
                                if guard_ty != Ty::Bool && !guard_ty.is_error() {
                                    self.error(
                                        ErrorCode::E0202,
                                        format!("match guard must be bool, got `{guard_ty}`"),
                                        guard.span.clone(),
                                    );
                                }
                            }
                            let ty = self.check_expr(&arm.body);
                            self.pop_scope();
                            ty
                        }
                        Pattern::Err(binding) => {
                            self.push_scope();
                            let err_ty = if let Ty::Result(_, ref err) = subject_ty {
                                *err.clone()
                            } else {
                                Ty::Error
                            };
                            self.define_var(
                                binding,
                                VarInfo {
                                    ty: err_ty,
                                    mutable: false,
                                },
                                &arm.pattern.span,
                            );
                            if let Some(ref guard) = arm.guard {
                                let guard_ty = self.check_expr(guard);
                                if guard_ty != Ty::Bool && !guard_ty.is_error() {
                                    self.error(
                                        ErrorCode::E0202,
                                        format!("match guard must be bool, got `{guard_ty}`"),
                                        guard.span.clone(),
                                    );
                                }
                            }
                            let ty = self.check_expr(&arm.body);
                            self.pop_scope();
                            ty
                        }
                        Pattern::Some(binding) => {
                            self.push_scope();
                            let inner_ty = if let Ty::Optional(ref inner) = subject_ty {
                                *inner.clone()
                            } else {
                                Ty::Error
                            };
                            self.define_var(
                                binding,
                                VarInfo {
                                    ty: inner_ty,
                                    mutable: false,
                                },
                                &arm.pattern.span,
                            );
                            if let Some(ref guard) = arm.guard {
                                let guard_ty = self.check_expr(guard);
                                if guard_ty != Ty::Bool && !guard_ty.is_error() {
                                    self.error(
                                        ErrorCode::E0202,
                                        format!("match guard must be bool, got `{guard_ty}`"),
                                        guard.span.clone(),
                                    );
                                }
                            }
                            let ty = self.check_expr(&arm.body);
                            self.pop_scope();
                            ty
                        }
                        Pattern::None => {
                            if let Some(ref guard) = arm.guard {
                                let guard_ty = self.check_expr(guard);
                                if guard_ty != Ty::Bool && !guard_ty.is_error() {
                                    self.error(
                                        ErrorCode::E0202,
                                        format!("match guard must be bool, got `{guard_ty}`"),
                                        guard.span.clone(),
                                    );
                                }
                            }
                            self.check_expr(&arm.body)
                        }
                        Pattern::VariantDestructure { variant, bindings } => {
                            self.push_scope();
                            if let Ty::Enum(ref enum_name) = subject_ty {
                                if let Some(info) = self.enums.get(enum_name).cloned() {
                                    if let Some(field_tys) = info.variant_fields(variant) {
                                        for (i, binding) in bindings.iter().enumerate() {
                                            let ty = if i < field_tys.len() {
                                                field_tys[i].clone()
                                            } else {
                                                Ty::Error
                                            };
                                            self.define_var(
                                                binding,
                                                VarInfo { ty, mutable: false },
                                                &arm.pattern.span,
                                            );
                                        }
                                    }
                                }
                            }
                            if let Some(ref guard) = arm.guard {
                                let guard_ty = self.check_expr(guard);
                                if guard_ty != Ty::Bool && !guard_ty.is_error() {
                                    self.error(
                                        ErrorCode::E0202,
                                        format!("match guard must be bool, got `{guard_ty}`"),
                                        guard.span.clone(),
                                    );
                                }
                            }
                            let ty = self.check_expr(&arm.body);
                            self.pop_scope();
                            ty
                        }
                        Pattern::Ident(name) if !matches!(subject_ty, Ty::Enum(_)) => {
                            // Non-enum ident pattern acts as a variable binding
                            self.push_scope();
                            self.define_var(
                                name,
                                VarInfo {
                                    ty: subject_ty.clone(),
                                    mutable: false,
                                },
                                &arm.pattern.span,
                            );
                            if let Some(ref guard) = arm.guard {
                                let guard_ty = self.check_expr(guard);
                                if guard_ty != Ty::Bool && !guard_ty.is_error() {
                                    self.error(
                                        ErrorCode::E0202,
                                        format!("match guard must be bool, got `{guard_ty}`"),
                                        guard.span.clone(),
                                    );
                                }
                            }
                            let ty = self.check_expr(&arm.body);
                            self.pop_scope();
                            ty
                        }
                        _ => {
                            // Enum ident patterns, wildcard, literal patterns
                            if let Some(ref guard) = arm.guard {
                                let guard_ty = self.check_expr(guard);
                                if guard_ty != Ty::Bool && !guard_ty.is_error() {
                                    self.error(
                                        ErrorCode::E0202,
                                        format!("match guard must be bool, got `{guard_ty}`"),
                                        guard.span.clone(),
                                    );
                                }
                            }
                            self.check_expr(&arm.body)
                        }
                    };

                    if let Some(ref expected) = result_ty {
                        if !body_ty.is_error() && !expected.is_error() && body_ty != *expected {
                            self.error(
                                ErrorCode::E0108,
                                format!(
                                    "match arms have different types: `{expected}` and `{body_ty}`"
                                ),
                                arm.body.span.clone(),
                            );
                        }
                    } else {
                        result_ty = Some(body_ty);
                    }
                }

                // Exhaustiveness check
                if !has_wildcard && !subject_ty.is_error() {
                    match &subject_ty {
                        Ty::Enum(enum_name) => {
                            if let Some(info) = self.enums.get(enum_name).cloned() {
                                let variant_names = info.variant_names();
                                let missing: Vec<&String> = variant_names
                                    .iter()
                                    .filter(|v| !covered_variants.contains(v))
                                    .collect();
                                if !missing.is_empty() {
                                    let missing_str: Vec<&str> =
                                        missing.iter().map(|s| s.as_str()).collect();
                                    self.error(
                                        ErrorCode::E0200,
                                        format!(
                                            "match is not exhaustive; missing variants: {}",
                                            missing_str.join(", ")
                                        ),
                                        expr.span.clone(),
                                    );
                                }
                            }
                        }
                        Ty::Bool => {
                            let has_true = covered_variants.contains(&"true".to_string());
                            let has_false = covered_variants.contains(&"false".to_string());
                            if !has_true || !has_false {
                                let mut missing = Vec::new();
                                if !has_true {
                                    missing.push("true");
                                }
                                if !has_false {
                                    missing.push("false");
                                }
                                self.error(
                                    ErrorCode::E0200,
                                    format!(
                                        "match is not exhaustive; missing variants: {}",
                                        missing.join(", ")
                                    ),
                                    expr.span.clone(),
                                );
                            }
                        }
                        Ty::Result(_, _) => {
                            let has_ok = covered_variants.contains(&"ok".to_string());
                            let has_err = covered_variants.contains(&"err".to_string());
                            if !has_ok || !has_err {
                                let mut missing = Vec::new();
                                if !has_ok {
                                    missing.push("ok");
                                }
                                if !has_err {
                                    missing.push("err");
                                }
                                self.error(
                                    ErrorCode::E0200,
                                    format!(
                                        "match is not exhaustive; missing variants: {}",
                                        missing.join(", ")
                                    ),
                                    expr.span.clone(),
                                );
                            }
                        }
                        Ty::Optional(_) => {
                            let has_some = covered_variants.contains(&"some".to_string());
                            let has_none = covered_variants.contains(&"none".to_string());
                            if !has_some || !has_none {
                                let mut missing = Vec::new();
                                if !has_some {
                                    missing.push("some");
                                }
                                if !has_none {
                                    missing.push("none");
                                }
                                self.error(
                                    ErrorCode::E0200,
                                    format!(
                                        "match is not exhaustive; missing variants: {}",
                                        missing.join(", ")
                                    ),
                                    expr.span.clone(),
                                );
                            }
                        }
                        _ => {
                            // For integers, strings, etc., a wildcard is required
                            self.error(
                                ErrorCode::E0200,
                                "match is not exhaustive; consider adding a wildcard `_` arm"
                                    .to_string(),
                                expr.span.clone(),
                            );
                        }
                    }
                }

                result_ty.unwrap_or(Ty::Unit)
            }

            Expr::Interpolation(parts) => {
                for part in parts {
                    if let InterpolPart::Expr(expr) = part {
                        self.check_expr(expr);
                    }
                }
                Ty::Str
            }

            Expr::Closure {
                params,
                return_type,
                body,
            } => {
                let mut param_types = Vec::new();
                let param_hint = self.closure_param_hint.take();
                self.push_scope();
                for (i, param) in params.iter().enumerate() {
                    let ty = if matches!(param.ty.node, TypeExpr::Inferred) {
                        if let Some(ref hints) = param_hint {
                            if i < hints.len() {
                                hints[i].clone()
                            } else {
                                self.error(
                                    ErrorCode::E0126,
                                    format!(
                                        "cannot infer type of closure parameter `{}`",
                                        param.name
                                    ),
                                    param.ty.span.clone(),
                                );
                                Ty::Error
                            }
                        } else {
                            self.error(ErrorCode::E0126,
                                format!("cannot infer type of closure parameter `{}` -- add a type annotation", param.name),
                                param.ty.span.clone(),
                            );
                            Ty::Error
                        }
                    } else {
                        match resolve_type_expr(
                            &param.ty.node,
                            Some(&self.structs),
                            Some(&self.enums),
                        ) {
                            Some(ty) => ty,
                            None => {
                                self.error(
                                    ErrorCode::E0305,
                                    format!("unknown type in closure parameter `{}`", param.name),
                                    param.ty.span.clone(),
                                );
                                Ty::Error
                            }
                        }
                    };
                    self.define_var(
                        &param.name,
                        VarInfo {
                            ty: ty.clone(),
                            mutable: false,
                        },
                        &param.span,
                    );
                    param_types.push(ty);
                }
                let body_ty = self.check_expr(body);
                self.pop_scope();

                let ret_ty = if let Some(rt) = return_type {
                    match resolve_type_expr(&rt.node, Some(&self.structs), Some(&self.enums)) {
                        Some(ty) => {
                            if !body_ty.is_error() && !ty.is_error() && body_ty != ty {
                                self.error(
                                    ErrorCode::E0125,
                                    format!(
                                        "closure body returns `{body_ty}` but return type is `{ty}`"
                                    ),
                                    body.span.clone(),
                                );
                            }
                            ty
                        }
                        None => {
                            self.error(
                                ErrorCode::E0127,
                                "unknown closure return type".to_string(),
                                rt.span.clone(),
                            );
                            body_ty
                        }
                    }
                } else {
                    body_ty
                };

                Ty::Fn(param_types, Box::new(ret_ty))
            }

            Expr::OkExpr(value) => {
                let val_ty = self.check_expr(value);
                // Return a partial result type -- the error type is unknown without context
                Ty::Result(Box::new(val_ty), Box::new(Ty::Error))
            }

            Expr::ErrExpr(value) => {
                let val_ty = self.check_expr(value);
                // Return a partial result type -- the ok type is unknown without context
                Ty::Result(Box::new(Ty::Error), Box::new(val_ty))
            }

            Expr::SomeExpr(value) => {
                let val_ty = self.check_expr(value);
                Ty::Optional(Box::new(val_ty))
            }

            Expr::NoneExpr => {
                // Return a partial optional type -- the inner type is unknown without context
                Ty::Optional(Box::new(Ty::Error))
            }

            Expr::Break => {
                if self.loop_depth == 0 {
                    self.error(
                        ErrorCode::E0507,
                        "`break` can only be used inside a loop".to_string(),
                        expr.span.clone(),
                    );
                }
                Ty::Unit
            }

            Expr::Continue => {
                if self.loop_depth == 0 {
                    self.error(
                        ErrorCode::E0508,
                        "`continue` can only be used inside a loop".to_string(),
                        expr.span.clone(),
                    );
                }
                Ty::Unit
            }

            Expr::NullCoalesce { value, default } => {
                let val_ty = self.check_expr(value);
                let def_ty = self.check_expr(default);

                if val_ty.is_error() || def_ty.is_error() {
                    return if def_ty.is_error() { Ty::Error } else { def_ty };
                }

                match &val_ty {
                    Ty::Optional(inner) => {
                        if !inner.is_error() && !def_ty.is_error() && **inner != def_ty {
                            self.error(ErrorCode::E0118,
                                format!(
                                    "`??` operator: optional inner type `{}` doesn't match default type `{def_ty}`",
                                    inner
                                ),
                                default.span.clone(),
                            );
                        }
                        if inner.is_error() {
                            def_ty
                        } else {
                            *inner.clone()
                        }
                    }
                    _ => {
                        self.error(ErrorCode::E0119,
                            format!("`??` operator requires an optional type on the left, found `{val_ty}`"),
                            value.span.clone(),
                        );
                        def_ty
                    }
                }
            }
        }
    }

    fn check_pattern(&mut self, pattern: &Spanned<Pattern>, subject_ty: &Ty) {
        match &pattern.node {
            Pattern::Wildcard => {
                // Wildcard matches anything
            }
            Pattern::Ident(name) => {
                // If subject is an enum, check that name is a valid variant
                if let Ty::Enum(enum_name) = subject_ty {
                    if let Some(info) = self.enums.get(enum_name) {
                        if !info.has_variant(name) {
                            self.error(
                                ErrorCode::E0316,
                                format!("enum `{enum_name}` has no variant `{name}`"),
                                pattern.span.clone(),
                            );
                        }
                    }
                }
                // For non-enum types, Ident pattern is treated as a variable binding
            }
            Pattern::IntLit(_) => {
                if !subject_ty.is_error() && !subject_ty.is_integer() {
                    self.error(
                        ErrorCode::E0132,
                        format!("integer pattern cannot match `{subject_ty}`"),
                        pattern.span.clone(),
                    );
                }
            }
            Pattern::BoolLit(_) => {
                if !subject_ty.is_error() && *subject_ty != Ty::Bool {
                    self.error(
                        ErrorCode::E0132,
                        format!("boolean pattern cannot match `{subject_ty}`"),
                        pattern.span.clone(),
                    );
                }
            }
            Pattern::StringLit(_) => {
                if !subject_ty.is_error() && *subject_ty != Ty::Str {
                    self.error(
                        ErrorCode::E0132,
                        format!("string pattern cannot match `{subject_ty}`"),
                        pattern.span.clone(),
                    );
                }
            }
            Pattern::Ok(_) => {
                if !subject_ty.is_error() && !matches!(subject_ty, Ty::Result(_, _)) {
                    self.error(
                        ErrorCode::E0132,
                        format!("ok pattern cannot match `{subject_ty}`"),
                        pattern.span.clone(),
                    );
                }
            }
            Pattern::Err(_) => {
                if !subject_ty.is_error() && !matches!(subject_ty, Ty::Result(_, _)) {
                    self.error(
                        ErrorCode::E0132,
                        format!("err pattern cannot match `{subject_ty}`"),
                        pattern.span.clone(),
                    );
                }
            }
            Pattern::Some(_) => {
                if !subject_ty.is_error() && !matches!(subject_ty, Ty::Optional(_)) {
                    self.error(
                        ErrorCode::E0132,
                        format!("some pattern cannot match `{subject_ty}`"),
                        pattern.span.clone(),
                    );
                }
            }
            Pattern::None => {
                if !subject_ty.is_error() && !matches!(subject_ty, Ty::Optional(_)) {
                    self.error(
                        ErrorCode::E0132,
                        format!("none pattern cannot match `{subject_ty}`"),
                        pattern.span.clone(),
                    );
                }
            }
            Pattern::VariantDestructure { variant, bindings } => {
                if let Ty::Enum(enum_name) = subject_ty {
                    if let Some(info) = self.enums.get(enum_name) {
                        if let Some(field_tys) = info.variant_fields(variant) {
                            if bindings.len() != field_tys.len() {
                                self.error(ErrorCode::E0100,
                                    format!(
                                        "variant `{variant}` has {} field(s) but pattern has {} binding(s)",
                                        field_tys.len(), bindings.len()
                                    ),
                                    pattern.span.clone(),
                                );
                            }
                        } else {
                            self.error(
                                ErrorCode::E0316,
                                format!("enum `{enum_name}` has no variant `{variant}`"),
                                pattern.span.clone(),
                            );
                        }
                    }
                } else if !subject_ty.is_error() {
                    self.error(
                        ErrorCode::E0132,
                        format!("variant destructure pattern cannot match `{subject_ty}`"),
                        pattern.span.clone(),
                    );
                }
            }
        }
    }

    fn check_stmt(&mut self, stmt: &Spanned<Stmt>) {
        match &stmt.node {
            Stmt::Let {
                mutable,
                name,
                ty,
                value,
            } => {
                let val_ty = self.check_expr(value);

                let declared_ty = if let Some(ty_expr) = ty {
                    match resolve_type_expr(&ty_expr.node, Some(&self.structs), Some(&self.enums)) {
                        Some(t) => {
                            if !val_ty.contains_error() && !types_compatible(&t, &val_ty) && t != val_ty {
                                // Allow integer literal coercion: i64 literal -> i32, u32, u64
                                let is_int_literal_coercion = val_ty == Ty::I64
                                    && matches!(t, Ty::I32 | Ty::U32 | Ty::U64)
                                    && matches!(&value.node, Expr::IntLit(n) if
                                        match t {
                                            Ty::U32 | Ty::U64 => *n >= 0,
                                            _ => true,
                                        }
                                    );
                                if !is_int_literal_coercion {
                                    self.error(ErrorCode::E0110,
                                        format!(
                                            "type annotation `{t}` doesn't match value type `{val_ty}`"
                                        ),
                                        ty_expr.span.clone(),
                                    );
                                }
                            }
                            t
                        }
                        None => {
                            if let TypeExpr::Named(name) = &ty_expr.node {
                                self.error(
                                    ErrorCode::E0305,
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
                    VarInfo {
                        ty: declared_ty,
                        mutable: *mutable,
                    },
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

                if !ret_ty.contains_error()
                    && !self.current_return_type.contains_error()
                    && self.current_return_type != Ty::Unit
                    && !types_compatible(&self.current_return_type, &ret_ty)
                    && ret_ty != self.current_return_type
                {
                    self.error(
                        ErrorCode::E0109,
                        format!(
                            "return type `{ret_ty}` doesn't match function return type `{}`",
                            self.current_return_type
                        ),
                        stmt.span.clone(),
                    );
                }
            }
            Stmt::Defer(expr) => {
                // Type-check the deferred expression (it should be a valid expression, typically a call)
                self.check_expr(expr);
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

/// Run semantic analysis in test mode (no `main` required). Returns errors found.
pub fn check_test(module: &Module) -> Vec<SemaError> {
    let mut checker = Checker::new();
    checker.test_mode = true;
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
            "type parameter `T` inferred as `i64` but argument has type `str`",
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
            "should return `i64` but body returns `str`",
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
        assert_no_errors(
            "fn get_val() -> i64? { none }\nfn main() { let x = get_val() }",
        );
    }
}
