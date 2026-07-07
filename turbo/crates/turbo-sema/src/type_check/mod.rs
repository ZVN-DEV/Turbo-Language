//! Type inference, name resolution, and statement / expression checking
//! for the Turbo semantic analyzer.
//!
//! This module is split across three files:
//!
//! * **`mod.rs`** (this file) — module-level checking, function checking,
//!   and small helpers:
//!   - `check_module` — top-level driver; registers types/functions and
//!     then walks every function body.
//!   - `check_function` / `check_function_with_name` — per-function scope
//!     setup and body checking.
//!   - `check_expr` — depth-limited entry point that delegates to
//!     `check_expr_inner`.
//!   - `is_builtin_function`, `root_var_name`, `substitute_ty`,
//!     `substitute_return_type`, `inject_constants`.
//!
//! * **[`expr`]** — `check_expr_inner`, the large match on every
//!   [`turbo_ast::Expr`] variant. This is the workhorse of expression-
//!   level type inference and validation.
//!
//! * **[`stmt`]** — `check_stmt`, statement-level checking (let, return,
//!   defer, destructure).
//!
//! Pattern-level checks live in [`crate::exhaustiveness`] and the scope
//! stack lives in [`crate::scope`]. Both are called from here via regular
//! method dispatch on `self: &mut Checker`.

use std::collections::HashMap;

use turbo_ast::*;

use crate::scope::VarInfo;
use crate::{
    erase_type_params, extract_int_literal, int_literal_fits_in_type, is_ffi_safe_type,
    resolve_type_expr, resolve_type_expr_with_params, types_compatible, Checker, EnumInfo, FnSig,
    StructInfo, TraitInfo, TraitMethodInfo, Ty, MAX_EXPR_DEPTH,
};

mod expr;
mod stmt;

/// Names recognized as built-in functions. Single source of truth for both the
/// `is_builtin_function` membership check and "did you mean" suggestions on an
/// undefined call.
pub(crate) const BUILTIN_FNS: &[&str] = &[
    "print",
    "panic",
    "assert",
    "assert_eq",
    "assert_ne",
    "len",
    "push",
    "abs",
    "min",
    "max",
    "to_str",
    "map",
    "filter",
    "reduce",
    "split",
    "trim",
    "upper",
    "lower",
    "starts_with",
    "ends_with",
    "replace",
    "char_at",
    "contains",
    "index_of",
    "join",
    "repeat",
    "read_line",
    "read_file",
    "write_file",
    "try_read_file",
    "try_write_file",
    "shell_exec",
    "exec",
    "env_get",
    "pow",
    "sqrt",
    "sleep",
    "http_get",
    "http_post",
    "http_post_with_headers",
    "json_get",
    "json_stringify",
    "json_build",
    "float_to_int",
    "int_to_float",
    "str_from_char",
    "http_server",
    "http_server_public",
    "route",
    "http_listen",
    "http_config",
    "respond",
    "respond_text",
    "respond_html",
    "respond_json",
    "request_body",
    "request_method",
    "request_path",
    "request_query",
    "request_header",
    "channel",
    "send",
    "recv",
    "mutex",
    "mutex_get",
    "mutex_set",
    "mutex_update",
    "clone",
    "hashmap",
    "hashmap_set",
    "hashmap_get",
    "hashmap_has",
    "hashmap_len",
    "hashmap_size",
    "hashmap_keys",
    "hashmap_remove",
    "hashmap_inc",
    "to_json",
    "to_json_array",
    "deref",
    "store",
    "args",
    "type_of",
    "random",
    "random_range",
    "substring",
    "pad_left",
    "pad_right",
    "str_to_int",
    "str_to_float",
    "file_exists",
    "delete_file",
    "list_dir",
    "mkdir",
    "path_join",
    "path_dir",
    "path_base",
    "path_ext",
    "sort",
    "reverse",
    "array_contains",
    "slice",
    "any",
    "all",
    "time_now",
    "time_ms",
    "format_time",
];

impl Checker {
    // === Check module ===

    pub(crate) fn is_builtin_function(name: &str) -> bool {
        BUILTIN_FNS.contains(&name)
    }

    /// Walk a chain of FieldAccess / Index expressions to find the root variable name.
    pub(crate) fn root_var_name(expr: &Spanned<Expr>) -> Option<String> {
        match &expr.node {
            Expr::Ident(name) => Some(name.clone()),
            Expr::FieldAccess { object, .. } | Expr::OptionalChain { object, .. } => {
                Self::root_var_name(object)
            }
            Expr::Index { object, .. } => Self::root_var_name(object),
            _ => None,
        }
    }

    pub(crate) fn check_module(&mut self, module: &Module) {
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
            let mut seen_fields: std::collections::HashSet<&str> = std::collections::HashSet::new();
            for field in &s.fields {
                if !seen_fields.insert(field.name.as_str()) {
                    self.error(
                        ErrorCode::E0306,
                        format!("duplicate field `{}` in struct `{}`", field.name, s.name),
                        field.ty.span.clone(),
                    );
                }
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
            let mut seen_params: std::collections::HashSet<&str> = std::collections::HashSet::new();
            for param in &f.params {
                if !seen_params.insert(param.name.as_str()) {
                    self.error(
                        ErrorCode::E0308,
                        format!(
                            "duplicate parameter `{}` in function `{}`",
                            param.name, f.name
                        ),
                        param.span.clone(),
                    );
                }
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

        // Pass 1a: register extern function signatures
        for item in &module.items {
            let Item::Extern(ext) = &item.node else {
                continue;
            };

            if ext.abi != "C" {
                self.error(
                    ErrorCode::E0001,
                    format!("unsupported ABI \"{}\"; only \"C\" is supported", ext.abi),
                    item.span.clone(),
                );
                continue;
            }

            for fn_sig in &ext.functions {
                let f = &fn_sig.node;

                // Check for duplicate definitions (user functions or builtins)
                if self.functions.contains_key(&f.name) || Self::is_builtin_function(&f.name) {
                    self.error(
                        ErrorCode::E0313,
                        format!("cannot define extern function `{}`: name is already defined or is a builtin", f.name),
                        fn_sig.span.clone(),
                    );
                    continue;
                }

                let mut params = Vec::new();
                for param in &f.params {
                    let ty =
                        resolve_type_expr(&param.ty.node, Some(&self.structs), Some(&self.enums))
                            .unwrap_or(Ty::Error);
                    if !is_ffi_safe_type(&ty) {
                        self.error(
                            ErrorCode::E0100,
                            format!("type `{ty}` is not FFI-safe; extern functions only support scalar types (int, float, i32, i64, f32, f64, u8, u16, u32, u64, bool, str)"),
                            param.ty.span.clone(),
                        );
                    }
                    params.push((param.name.clone(), ty));
                }

                let ret = if let Some(ret_type) = &f.return_type {
                    let ty =
                        resolve_type_expr(&ret_type.node, Some(&self.structs), Some(&self.enums))
                            .unwrap_or(Ty::Error);
                    if !is_ffi_safe_type(&ty) {
                        self.error(
                            ErrorCode::E0100,
                            format!("return type `{ty}` is not FFI-safe; extern functions only support scalar types"),
                            ret_type.span.clone(),
                        );
                    }
                    ty
                } else {
                    Ty::Unit
                };

                self.functions.insert(
                    f.name.clone(),
                    FnSig {
                        type_params: vec![],
                        type_param_bounds: HashMap::new(),
                        params,
                        ret,
                        is_async: false,
                        is_unsafe: false,
                    },
                );
                // Record this as an explicit FFI declaration so call-site type
                // checking prefers it over any same-named native builtin (e.g.
                // the int-returning `floor`/`ceil` math builtins).
                self.extern_fns.insert(f.name.clone());
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
            let inferred_ty = match &c.value.node {
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
            // Validate optional type annotation
            let ty = if let Some(ty_expr) = &c.ty {
                match resolve_type_expr(&ty_expr.node, Some(&self.structs), Some(&self.enums)) {
                    Some(t) => {
                        if !inferred_ty.contains_error()
                            && !types_compatible(&t, &inferred_ty)
                            && t != inferred_ty
                        {
                            // Allow integer literal coercion: i64 literal -> narrower int types
                            let is_int_literal_coercion = inferred_ty == Ty::I64
                                && matches!(
                                    t,
                                    Ty::I8
                                        | Ty::I16
                                        | Ty::I32
                                        | Ty::U8
                                        | Ty::U16
                                        | Ty::U32
                                        | Ty::U64
                                )
                                && extract_int_literal(&c.value.node)
                                    .is_some_and(|n| int_literal_fits_in_type(n, &t));
                            if !is_int_literal_coercion {
                                let annotation = crate::type_annotation_label(&ty_expr.node, &t);
                                self.error(
                                    ErrorCode::E0110,
                                    format!(
                                        "type annotation `{annotation}` doesn't match value type `{inferred_ty}`"
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
                        inferred_ty
                    }
                }
            } else {
                inferred_ty
            };
            self.constants.insert(c.name.clone(), ty);
        }

        // Check for main when using the binary/program entry point.
        if self.require_main && !self.functions.contains_key("main") {
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
    pub(crate) fn substitute_ty(&self, ty: &Ty, subs: &HashMap<String, Ty>) -> Ty {
        match ty {
            Ty::TypeParam(name) => subs.get(name).cloned().unwrap_or_else(|| ty.clone()),
            other => other.clone(),
        }
    }

    /// Get the concrete return type of a function given substitutions
    pub(crate) fn substitute_return_type(&self, sig: &FnSig, subs: &HashMap<String, Ty>) -> Ty {
        self.substitute_ty(&sig.ret, subs)
    }

    /// Inject module-level constants into the current scope as immutable variables.
    pub(crate) fn inject_constants(&mut self) {
        let consts: Vec<(String, Ty)> = self
            .constants
            .iter()
            .map(|(n, t)| (n.clone(), t.clone()))
            .collect();
        for (name, ty) in consts {
            self.define_var(
                &name,
                VarInfo {
                    ty,
                    mutable: false,
                    span: 0..0,
                    from_let: false,
                },
                &(0..0),
            );
        }
    }

    pub(crate) fn check_function(&mut self, f: &FnDef) {
        let sig = match self.functions.get(&f.name).cloned() {
            Some(s) => s,
            None => {
                // Function not registered (e.g. shadowed builtin or internal error).
                // Skip body checking to avoid a panic.
                return;
            }
        };

        // Generic functions are still body-checked, but each type parameter is
        // erased to `Ty::Error` first. `Error` is the checker's poison type: any
        // expression that touches it is exempt from further checks, so we don't
        // emit false positives on legitimate generic operations (`x + 1` where
        // `x: T`). Concrete bugs that don't involve a type parameter — undefined
        // names, `let y: i64 = "string"`, a concrete-typed tail that disagrees
        // with the return type — still surface normally. Call-site inference
        // (which uses the un-erased signature) is unaffected.
        let is_generic = !sig.type_params.is_empty();
        let param_tys: Vec<Ty> = if is_generic {
            sig.params
                .iter()
                .map(|(_, t)| erase_type_params(t))
                .collect()
        } else {
            sig.params.iter().map(|(_, t)| t.clone()).collect()
        };
        let ret_ty = if is_generic {
            erase_type_params(&sig.ret)
        } else {
            sig.ret.clone()
        };

        self.current_return_type = ret_ty.clone();
        let prev_unsafe = self.in_unsafe_context;
        self.in_unsafe_context = f.is_unsafe;

        self.push_scope();

        // Inject module-level constants
        self.inject_constants();

        // Define parameters (type-param-erased for generic functions, see above)
        for (i, (name, _)) in sig.params.iter().enumerate() {
            let (param_span, param_mutable) = if i < f.params.len() {
                (f.params[i].span.clone(), f.params[i].mutable)
            } else {
                (0..0, false)
            };
            self.define_var(
                name,
                VarInfo {
                    ty: param_tys[i].clone(),
                    mutable: param_mutable,
                    span: param_span,
                    from_let: false,
                },
                &(0..0),
            );
        }

        // Check body. Hint the declared return type so an empty-array tail
        // expression (`fn f() -> [str] { [] }`) infers its element type from
        // the return slot (BL-26); `check_block` consumes the hint.
        self.fn_body_tail_hint = Some(ret_ty.clone());
        let body_ty = self.check_expr(&f.body);
        self.fn_body_tail_hint = None;

        // Check return type matches.
        // Skip if the body ends with a `return` statement — those are checked
        // individually in Stmt::Return, and the block's own type is Unit (which
        // would falsely trigger this error).
        let body_has_return = if let Expr::Block { stmts, .. } = &f.body.node {
            stmts
                .last()
                .is_some_and(|s| matches!(s.node, Stmt::Return(_)))
        } else {
            false
        };
        if !ret_ty.is_error()
            && !body_ty.is_error()
            && ret_ty != Ty::Unit
            && !types_compatible(&ret_ty, &body_ty)
            && !body_has_return
        {
            // Allow integer literal coercion for tail expression (e.g., fn foo() -> u8 { 42 })
            let tail_expr = if let Expr::Block { tail_expr, .. } = &f.body.node {
                tail_expr.as_ref().map(|t| &t.node)
            } else {
                None
            };
            let is_return_coercion = body_ty == Ty::I64
                && matches!(
                    ret_ty,
                    Ty::I8 | Ty::I16 | Ty::I32 | Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64
                )
                && tail_expr
                    .and_then(extract_int_literal)
                    .is_some_and(|n| int_literal_fits_in_type(n, &ret_ty));
            if !is_return_coercion {
                self.error(
                    ErrorCode::E0109,
                    format!(
                        "function `{}` should return `{}` but body returns `{}`",
                        f.name, ret_ty, body_ty
                    ),
                    f.body.span.clone(),
                );
            }
        }

        self.pop_scope();
        self.in_unsafe_context = prev_unsafe;
    }

    /// Check a function body using a different name for looking up its signature (for methods).
    pub(crate) fn check_function_with_name(&mut self, f: &FnDef, sig_name: &str) {
        let sig = match self.functions.get(sig_name).cloned() {
            Some(s) => s,
            None => {
                // Method signature not registered (internal error or race condition).
                // Skip body checking to avoid a panic.
                return;
            }
        };
        self.current_return_type = sig.ret.clone();

        self.push_scope();

        // Inject module-level constants
        self.inject_constants();

        for (i, (name, ty)) in sig.params.iter().enumerate() {
            let (param_span, param_mutable) = if i < f.params.len() {
                (f.params[i].span.clone(), f.params[i].mutable)
            } else {
                (0..0, false)
            };
            self.define_var(
                name,
                VarInfo {
                    ty: ty.clone(),
                    mutable: param_mutable,
                    span: param_span,
                    from_let: false,
                },
                &(0..0),
            );
        }

        // Hint the declared return type for an empty-array tail return (BL-26);
        // `check_block` consumes it.
        self.fn_body_tail_hint = Some(sig.ret.clone());
        let body_ty = self.check_expr(&f.body);
        self.fn_body_tail_hint = None;

        let body_has_return = if let Expr::Block { stmts, .. } = &f.body.node {
            stmts
                .last()
                .is_some_and(|s| matches!(s.node, Stmt::Return(_)))
        } else {
            false
        };
        if !sig.ret.is_error()
            && !body_ty.is_error()
            && sig.ret != Ty::Unit
            && !types_compatible(&sig.ret, &body_ty)
            && !body_has_return
        {
            // Allow integer literal coercion for tail expression
            let tail_expr = if let Expr::Block { tail_expr, .. } = &f.body.node {
                tail_expr.as_ref().map(|t| &t.node)
            } else {
                None
            };
            let is_return_coercion = body_ty == Ty::I64
                && matches!(
                    sig.ret,
                    Ty::I8 | Ty::I16 | Ty::I32 | Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64
                )
                && tail_expr
                    .and_then(extract_int_literal)
                    .is_some_and(|n| int_literal_fits_in_type(n, &sig.ret));
            if !is_return_coercion {
                self.error(
                    ErrorCode::E0109,
                    format!(
                        "method `{}` should return `{}` but body returns `{}`",
                        f.name, sig.ret, body_ty
                    ),
                    f.body.span.clone(),
                );
            }
        }

        self.pop_scope();
    }

    // === Expression type checking ===

    pub(crate) fn check_expr(&mut self, expr: &Spanned<Expr>) -> Ty {
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

    /// Check an expression that appears in a position with a known expected
    /// type — a function parameter, struct field, or return slot.
    ///
    /// The only behavioural difference from [`check_expr`] is that a bare
    /// empty array literal `[]` infers its element type from `expected`
    /// instead of failing with `E0115` ("cannot infer type of empty array").
    /// This mirrors the let-binding special case (`let xs: [int] = []`) for
    /// the other annotated contexts. Everything else is unchanged: callers
    /// still compare the returned type against what they expect, and a `[]`
    /// flowing into a non-array or still-generic slot (e.g. a `[T]` parameter
    /// whose `T` is unbound) keeps the genuine E0115 diagnostic.
    pub(crate) fn check_expr_expecting(&mut self, expr: &Spanned<Expr>, expected: &Ty) -> Ty {
        if let Expr::ArrayLit(elems) = &expr.node {
            // Only an *empty* literal needs a hint; a non-empty one infers
            // its element type from its first element as usual.
            if elems.is_empty()
                && matches!(expected, Ty::Array(_))
                // The expected element type must be fully concrete. A `[T]`
                // (or any type still mentioning a generic parameter) is
                // genuinely uninferrable from `[]`, so let it fall through to
                // the normal E0115 path. `erase_type_params` rewrites every
                // `TypeParam` to `Error`; if that leaves `expected` unchanged
                // there were no type parameters to resolve.
                && erase_type_params(expected) == *expected
            {
                return expected.clone();
            }
        }
        self.check_expr(expr)
    }
}
