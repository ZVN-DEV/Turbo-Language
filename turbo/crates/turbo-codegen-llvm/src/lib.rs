//! LLVM backend for the Turbo language compiler.
//!
//! Uses inkwell (LLVM 18) to compile Turbo AST to native code via LLVM IR.
//! Mirrors the Cranelift backend's semantics for all supported AST nodes.

use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
#[allow(unused_imports)]
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FunctionType};
use inkwell::values::{
    BasicMetadataValueEnum, BasicValueEnum, FunctionValue, IntValue, PointerValue,
};
use inkwell::{AddressSpace, FloatPredicate, IntPredicate, OptimizationLevel};

use std::collections::HashMap;
use std::path::Path;
use turbo_ast::*;

// ── Runtime C source for AOT linking ────────────────────────────────

const RUNTIME_C: &str = include_str!("../../turbo-codegen-cranelift/runtime/turbo_rt.c");

// ── Error type ──────────────────────────────────────────────────────

#[derive(Debug)]
pub struct CodegenError {
    pub code: ErrorCode,
    pub message: String,
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "codegen error: {}", self.message)
    }
}

impl std::error::Error for CodegenError {}

// ── Turbo-level type tag ────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
enum TurboTy {
    Int,
    Float,
    Bool,
    Str,
    Unit,
    Array(Box<TurboTy>),
    Struct(String),
    Enum(String),
    Fn(Vec<TurboTy>, Box<TurboTy>),
    Result(Box<TurboTy>, Box<TurboTy>),
    Optional(Box<TurboTy>),
    Agent(String),
    Future(Box<TurboTy>),
}

type Typed<'ctx> = (BasicValueEnum<'ctx>, TurboTy);
type MaybeTyped<'ctx> = Option<Typed<'ctx>>;

// ── Type conversion helpers ─────────────────────────────────────────

fn turbo_ty_from_type_expr(te: &TypeExpr, enum_variants: &HashMap<String, Vec<String>>) -> TurboTy {
    turbo_ty_from_type_expr_with_params(te, enum_variants, &[])
}

fn turbo_ty_from_type_expr_with_params(
    te: &TypeExpr,
    enum_variants: &HashMap<String, Vec<String>>,
    type_params: &[String],
) -> TurboTy {
    match te {
        TypeExpr::Named(name) => {
            if type_params.contains(name) {
                return TurboTy::Int;
            }
            match name.as_str() {
                "i32" | "i64" | "u32" | "u64" => TurboTy::Int,
                "f32" | "f64" => TurboTy::Float,
                "bool" => TurboTy::Bool,
                "str" => TurboTy::Str,
                _ => {
                    if enum_variants.contains_key(name.as_str()) {
                        TurboTy::Enum(name.clone())
                    } else {
                        TurboTy::Struct(name.clone())
                    }
                }
            }
        }
        TypeExpr::Unit => TurboTy::Unit,
        TypeExpr::Array(inner) => TurboTy::Array(Box::new(turbo_ty_from_type_expr(
            &inner.node,
            enum_variants,
        ))),
        TypeExpr::FnType { params, ret } => {
            let param_tys: Vec<TurboTy> = params
                .iter()
                .map(|p| turbo_ty_from_type_expr(&p.node, enum_variants))
                .collect();
            let ret_ty = turbo_ty_from_type_expr(&ret.node, enum_variants);
            TurboTy::Fn(param_tys, Box::new(ret_ty))
        }
        TypeExpr::Result { ok_type, err_type } => {
            let ok_tty = turbo_ty_from_type_expr(&ok_type.node, enum_variants);
            let err_tty = turbo_ty_from_type_expr(&err_type.node, enum_variants);
            TurboTy::Result(Box::new(ok_tty), Box::new(err_tty))
        }
        TypeExpr::Optional(inner) => TurboTy::Optional(Box::new(turbo_ty_from_type_expr(
            &inner.node,
            enum_variants,
        ))),
        TypeExpr::Future(inner) => TurboTy::Future(Box::new(turbo_ty_from_type_expr_with_params(
            &inner.node,
            enum_variants,
            type_params,
        ))),
        _ => TurboTy::Int,
    }
}

/// Convert a TurboTy to an LLVM BasicTypeEnum.
fn turbo_ty_to_llvm<'ctx>(tty: &TurboTy, context: &'ctx Context) -> BasicTypeEnum<'ctx> {
    match tty {
        TurboTy::Int => context.i64_type().into(),
        TurboTy::Float => context.f64_type().into(),
        TurboTy::Bool => context.i8_type().into(),
        TurboTy::Str => context.ptr_type(AddressSpace::default()).into(),
        TurboTy::Unit => context.i64_type().into(),
        TurboTy::Fn(_, _) => context.ptr_type(AddressSpace::default()).into(),
        TurboTy::Array(_) => context.ptr_type(AddressSpace::default()).into(),
        TurboTy::Struct(_) => context.ptr_type(AddressSpace::default()).into(),
        // NOTE: Enum types are i64 for unit-only enums. For data-carrying enums,
        // use turbo_ty_to_llvm_with_enums() which checks enum_max_slots.
        TurboTy::Enum(_) => context.i64_type().into(),
        TurboTy::Result(_, _) => context.ptr_type(AddressSpace::default()).into(),
        TurboTy::Optional(_) => context.ptr_type(AddressSpace::default()).into(),
        TurboTy::Agent(_) => context.ptr_type(AddressSpace::default()).into(),
        TurboTy::Future(_) => context.ptr_type(AddressSpace::default()).into(),
    }
}

/// Resolve a TypeExpr to a TurboTy, then to an LLVM type.
#[allow(dead_code)]
fn resolve_llvm_type<'ctx>(
    ty: &TypeExpr,
    context: &'ctx Context,
    enum_variants: &HashMap<String, Vec<String>>,
    type_params: &[String],
) -> BasicTypeEnum<'ctx> {
    let tty = turbo_ty_from_type_expr_with_params(ty, enum_variants, type_params);
    turbo_ty_to_llvm(&tty, context)
}

/// Like turbo_ty_to_llvm but correctly handles data-carrying enums as pointers.
fn turbo_ty_to_llvm_ctx<'ctx>(
    tty: &TurboTy,
    context: &'ctx Context,
    enum_max_slots: &HashMap<String, usize>,
) -> BasicTypeEnum<'ctx> {
    if let TurboTy::Enum(ref name) = tty {
        if enum_max_slots.contains_key(name) {
            return context.ptr_type(AddressSpace::default()).into();
        }
    }
    turbo_ty_to_llvm(tty, context)
}

/// Resolve a TypeExpr to an LLVM type, data-enum-aware.
fn resolve_llvm_type_ctx<'ctx>(
    ty: &TypeExpr,
    context: &'ctx Context,
    enum_variants: &HashMap<String, Vec<String>>,
    enum_max_slots: &HashMap<String, usize>,
    type_params: &[String],
) -> BasicTypeEnum<'ctx> {
    let tty = turbo_ty_from_type_expr_with_params(ty, enum_variants, type_params);
    turbo_ty_to_llvm_ctx(&tty, context, enum_max_slots)
}

// ── Codegen context ─────────────────────────────────────────────────

#[allow(dead_code)]
struct Ctx<'a, 'ctx> {
    context: &'ctx Context,
    module: &'a Module<'ctx>,
    builder: &'a Builder<'ctx>,
    /// Currently compiled function
    current_fn: FunctionValue<'ctx>,
    /// User-defined functions
    user_fns: &'a HashMap<String, FunctionValue<'ctx>>,
    /// Function return types
    fn_ret_types: &'a HashMap<String, TurboTy>,
    /// Function ASTs (for inlining)
    fn_asts: &'a HashMap<String, &'a FnDef>,
    /// Function type params
    fn_type_params: &'a HashMap<String, Vec<String>>,
    /// Runtime functions
    rt_fns: &'a HashMap<String, FunctionValue<'ctx>>,
    /// Variable allocas: name -> (alloca ptr, turbo type)
    vars: HashMap<String, (PointerValue<'ctx>, TurboTy)>,
    /// String literal counter for unique names
    string_counter: &'a mut usize,
    /// Struct field layouts
    struct_fields: &'a HashMap<String, Vec<(String, TurboTy)>>,
    /// Enum variant lists
    enum_variants: &'a HashMap<String, Vec<String>>,
    /// Data-carrying enum variant fields
    enum_variant_fields: &'a HashMap<(String, String), Vec<TurboTy>>,
    /// Max slots per data enum
    enum_max_slots: &'a HashMap<String, usize>,
    /// Module-level constants
    constants: &'a HashMap<String, Spanned<Expr>>,
    /// Loop stack for break/continue: (header_block, exit_block)
    loop_stack: Vec<(
        inkwell::basic_block::BasicBlock<'ctx>,
        inkwell::basic_block::BasicBlock<'ctx>,
    )>,
    /// Closure functions: span_start -> (fn_name, TurboTy::Fn, free_var_names)
    closure_fns: &'a HashMap<usize, (String, TurboTy, Vec<String>)>,
    /// Spawn thunks: span_start -> thunk_fn_name
    spawn_thunks: &'a HashMap<usize, String>,
    /// Struct derives: struct_name -> vec of trait names
    struct_derives: &'a HashMap<String, Vec<String>>,
    /// Trait impls: type_name -> vec of trait names
    trait_impls: &'a HashMap<String, Vec<String>>,
    /// Agent names (to distinguish from regular structs)
    agent_names: &'a std::collections::HashSet<String>,
    /// Agent definitions: name -> (model, tools, system_prompt)
    agent_defs: &'a HashMap<String, (String, Vec<String>, Option<String>)>,
    /// Concrete field types for generic struct instances: var_name -> [(field, type)]
    concrete_struct_fields: HashMap<String, Vec<(String, TurboTy)>>,
}

impl<'a, 'ctx> Ctx<'a, 'ctx> {
    fn create_string(&mut self, s: &str) -> Result<PointerValue<'ctx>, CodegenError> {
        let name = format!(".str.{}", *self.string_counter);
        *self.string_counter += 1;
        let val = self
            .builder
            .build_global_string_ptr(s, &name)
            .map_err(|e| CodegenError {
                code: ErrorCode::E0405,
                message: e.to_string(),
            })?;
        Ok(val.as_pointer_value())
    }

    fn rt_call(
        &self,
        name: &str,
        args: &[BasicMetadataValueEnum<'ctx>],
    ) -> Option<BasicValueEnum<'ctx>> {
        let func = self.rt_fns[name];
        let call = self
            .builder
            .build_direct_call(func, args, "")
            .expect("build_direct_call failed");
        call.try_as_basic_value().left()
    }

    /// Convert a value to i1 boolean for use in conditional branches.
    /// LLVM requires `i1` for branch conditions, unlike Cranelift which uses `i8`.
    fn to_bool(&self, val: BasicValueEnum<'ctx>) -> IntValue<'ctx> {
        match val {
            BasicValueEnum::IntValue(iv) => {
                let ty = iv.get_type();
                let zero = ty.const_int(0, false);
                self.builder
                    .build_int_compare(IntPredicate::NE, iv, zero, "tobool")
                    .expect("build_int_compare failed")
            }
            _ => {
                // For non-int types, assume truthy
                self.context.bool_type().const_int(1, false)
            }
        }
    }

    /// Create an alloca in the entry block of the current function.
    fn create_entry_block_alloca(&self, ty: BasicTypeEnum<'ctx>, name: &str) -> PointerValue<'ctx> {
        let entry = self.current_fn.get_first_basic_block().unwrap();
        let builder = self.context.create_builder();
        match entry.get_first_instruction() {
            Some(first_instr) => builder.position_before(&first_instr),
            None => builder.position_at_end(entry),
        }
        builder.build_alloca(ty, name).expect("build_alloca failed")
    }
}

// ── Closure extraction ──────────────────────────────────────────────

struct ExtractedClosure<'a> {
    span_start: usize,
    name: String,
    params: &'a [Param],
    return_type: &'a Option<Spanned<TypeExpr>>,
    body: &'a Spanned<Expr>,
    free_vars: Vec<String>,
    /// Types of captured (free) variables, inferred from enclosing scope
    capture_types: Vec<TurboTy>,
}

/// Infer the type of a captured variable from how it's used in the closure body.
/// Checks if the variable appears in string interpolation (→ Str) or string concat (→ Str).
fn infer_capture_type_from_body(body: &Expr, var_name: &str) -> TurboTy {
    match body {
        Expr::Interpolation(parts) => {
            for part in parts {
                if let InterpolPart::Expr(e) = part {
                    if let Expr::Ident(name) = &e.node {
                        if name == var_name {
                            return TurboTy::Str;
                        }
                    }
                }
            }
            // Recurse into sub-expressions
            for part in parts {
                if let InterpolPart::Expr(e) = part {
                    let t = infer_capture_type_from_body(&e.node, var_name);
                    if t != TurboTy::Int {
                        return t;
                    }
                }
            }
            TurboTy::Int
        }
        Expr::Block { stmts, tail_expr } => {
            for stmt in stmts {
                match &stmt.node {
                    Stmt::Let { value, .. } => {
                        let t = infer_capture_type_from_body(&value.node, var_name);
                        if t != TurboTy::Int {
                            return t;
                        }
                    }
                    Stmt::Expr(e) => {
                        let t = infer_capture_type_from_body(&e.node, var_name);
                        if t != TurboTy::Int {
                            return t;
                        }
                    }
                    Stmt::Return(Some(e)) => {
                        let t = infer_capture_type_from_body(&e.node, var_name);
                        if t != TurboTy::Int {
                            return t;
                        }
                    }
                    _ => {}
                }
            }
            if let Some(tail) = tail_expr {
                return infer_capture_type_from_body(&tail.node, var_name);
            }
            TurboTy::Int
        }
        Expr::Call { callee, args } => {
            // If passed to rt_str_concat or similar, it's a string
            for arg in args {
                let t = infer_capture_type_from_body(&arg.node, var_name);
                if t != TurboTy::Int {
                    return t;
                }
            }
            infer_capture_type_from_body(&callee.node, var_name)
        }
        Expr::BinaryOp { left, right, .. } => {
            let t = infer_capture_type_from_body(&left.node, var_name);
            if t != TurboTy::Int {
                return t;
            }
            infer_capture_type_from_body(&right.node, var_name)
        }
        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => {
            let t = infer_capture_type_from_body(&condition.node, var_name);
            if t != TurboTy::Int {
                return t;
            }
            let t = infer_capture_type_from_body(&then_branch.node, var_name);
            if t != TurboTy::Int {
                return t;
            }
            if let Some(e) = else_branch {
                return infer_capture_type_from_body(&e.node, var_name);
            }
            TurboTy::Int
        }
        _ => TurboTy::Int,
    }
}

fn collect_free_vars_llvm(expr: &Expr, bound: &mut Vec<String>, free: &mut Vec<String>) {
    match expr {
        Expr::Ident(name) => {
            if !bound.contains(name) && !free.contains(name) {
                free.push(name.clone());
            }
        }
        // Other nodes don't bind names; handled by sub-expression walk below
        _ => {}
    }
    // Walk sub-expressions
    match expr {
        Expr::BinaryOp { left, right, .. } => {
            collect_free_vars_llvm(&left.node, bound, free);
            collect_free_vars_llvm(&right.node, bound, free);
        }
        Expr::UnaryOp { expr: e, .. } => collect_free_vars_llvm(&e.node, bound, free),
        Expr::Call { callee, args } => {
            collect_free_vars_llvm(&callee.node, bound, free);
            for arg in args {
                collect_free_vars_llvm(&arg.node, bound, free);
            }
        }
        Expr::Block { stmts, tail_expr } => {
            let prev_len = bound.len();
            for stmt in stmts {
                match &stmt.node {
                    Stmt::Let { name, value, .. } => {
                        collect_free_vars_llvm(&value.node, bound, free);
                        bound.push(name.clone());
                    }
                    Stmt::LetDestructure { fields, value, .. } => {
                        collect_free_vars_llvm(&value.node, bound, free);
                        for field_name in fields {
                            bound.push(field_name.clone());
                        }
                    }
                    Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Defer(e) => {
                        collect_free_vars_llvm(&e.node, bound, free);
                    }
                    Stmt::Return(None) => {}
                }
            }
            if let Some(tail) = tail_expr {
                collect_free_vars_llvm(&tail.node, bound, free);
            }
            bound.truncate(prev_len);
        }
        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_free_vars_llvm(&condition.node, bound, free);
            collect_free_vars_llvm(&then_branch.node, bound, free);
            if let Some(e) = else_branch {
                collect_free_vars_llvm(&e.node, bound, free);
            }
        }
        Expr::While { condition, body } => {
            collect_free_vars_llvm(&condition.node, bound, free);
            collect_free_vars_llvm(&body.node, bound, free);
        }
        Expr::ForIn {
            var_name,
            iterable,
            body,
        } => {
            collect_free_vars_llvm(&iterable.node, bound, free);
            let prev = bound.len();
            bound.push(var_name.clone());
            collect_free_vars_llvm(&body.node, bound, free);
            bound.truncate(prev);
        }
        Expr::Assign { target, value } | Expr::CompoundAssign { target, value, .. } => {
            if !bound.contains(target) && !free.contains(target) {
                free.push(target.clone());
            }
            collect_free_vars_llvm(&value.node, bound, free);
        }
        Expr::FieldAssign { object, value, .. } => {
            collect_free_vars_llvm(&object.node, bound, free);
            collect_free_vars_llvm(&value.node, bound, free);
        }
        Expr::IndexAssign {
            object,
            index,
            value,
        } => {
            collect_free_vars_llvm(&object.node, bound, free);
            collect_free_vars_llvm(&index.node, bound, free);
            collect_free_vars_llvm(&value.node, bound, free);
        }
        Expr::FieldAccess { object, .. } | Expr::OptionalChain { object, .. } => collect_free_vars_llvm(&object.node, bound, free),
        Expr::Index { object, index } => {
            collect_free_vars_llvm(&object.node, bound, free);
            collect_free_vars_llvm(&index.node, bound, free);
        }
        Expr::ArrayLit(elems) => {
            for e in elems {
                collect_free_vars_llvm(&e.node, bound, free);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, e) in fields {
                collect_free_vars_llvm(&e.node, bound, free);
            }
        }
        Expr::Match { subject, arms } => {
            collect_free_vars_llvm(&subject.node, bound, free);
            for arm in arms {
                if let Some(ref g) = arm.guard {
                    collect_free_vars_llvm(&g.node, bound, free);
                }
                collect_free_vars_llvm(&arm.body.node, bound, free);
            }
        }
        Expr::Closure { params, body, .. } => {
            let prev = bound.len();
            for p in params {
                bound.push(p.name.clone());
            }
            collect_free_vars_llvm(&body.node, bound, free);
            bound.truncate(prev);
        }
        Expr::OkExpr(v)
        | Expr::ErrExpr(v)
        | Expr::SomeExpr(v)
        | Expr::Await(v)
        | Expr::Spawn(v)
        | Expr::Try(v) => {
            collect_free_vars_llvm(&v.node, bound, free);
        }
        Expr::NullCoalesce { value, default } => {
            collect_free_vars_llvm(&value.node, bound, free);
            collect_free_vars_llvm(&default.node, bound, free);
        }
        Expr::Interpolation(parts) => {
            for part in parts {
                if let InterpolPart::Expr(e) = part {
                    collect_free_vars_llvm(&e.node, bound, free);
                }
            }
        }
        Expr::Range { start, end } => {
            collect_free_vars_llvm(&start.node, bound, free);
            collect_free_vars_llvm(&end.node, bound, free);
        }
        _ => {}
    }
}

fn extract_closures_from_expr_llvm<'a>(
    expr: &'a Spanned<Expr>,
    out: &mut Vec<ExtractedClosure<'a>>,
    counter: &mut usize,
) {
    match &expr.node {
        Expr::Closure {
            params,
            return_type,
            body,
        } => {
            let name = format!("__closure_{}", *counter);
            *counter += 1;
            let mut bound: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
            let mut free_vars = Vec::new();
            collect_free_vars_llvm(&body.node, &mut bound, &mut free_vars);
            // Infer capture types from body usage: scan for string interpolation/concat
            let capture_types: Vec<TurboTy> = free_vars
                .iter()
                .map(|var_name| infer_capture_type_from_body(&body.node, var_name))
                .collect();
            out.push(ExtractedClosure {
                span_start: expr.span.start,
                name,
                params,
                return_type,
                body,
                free_vars,
                capture_types,
            });
            extract_closures_from_expr_llvm(body, out, counter);
        }
        Expr::Block { stmts, tail_expr } => {
            for stmt in stmts {
                match &stmt.node {
                    Stmt::Let { value, .. }
                    | Stmt::LetDestructure { value, .. }
                    | Stmt::Expr(value) => {
                        extract_closures_from_expr_llvm(value, out, counter);
                    }
                    Stmt::Return(Some(e)) | Stmt::Defer(e) => {
                        extract_closures_from_expr_llvm(e, out, counter);
                    }
                    _ => {}
                }
            }
            if let Some(tail) = tail_expr {
                extract_closures_from_expr_llvm(tail, out, counter);
            }
        }
        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => {
            extract_closures_from_expr_llvm(condition, out, counter);
            extract_closures_from_expr_llvm(then_branch, out, counter);
            if let Some(e) = else_branch {
                extract_closures_from_expr_llvm(e, out, counter);
            }
        }
        Expr::While { condition, body }
        | Expr::ForIn {
            iterable: condition,
            body,
            ..
        } => {
            extract_closures_from_expr_llvm(condition, out, counter);
            extract_closures_from_expr_llvm(body, out, counter);
        }
        Expr::BinaryOp { left, right, .. } => {
            extract_closures_from_expr_llvm(left, out, counter);
            extract_closures_from_expr_llvm(right, out, counter);
        }
        Expr::UnaryOp { expr: e, .. } => extract_closures_from_expr_llvm(e, out, counter),
        Expr::Call { callee, args } => {
            extract_closures_from_expr_llvm(callee, out, counter);
            for arg in args {
                extract_closures_from_expr_llvm(arg, out, counter);
            }
        }
        Expr::Assign { value, .. } | Expr::CompoundAssign { value, .. } => {
            extract_closures_from_expr_llvm(value, out, counter);
        }
        Expr::FieldAssign { object, value, .. } => {
            extract_closures_from_expr_llvm(object, out, counter);
            extract_closures_from_expr_llvm(value, out, counter);
        }
        Expr::IndexAssign {
            object,
            index,
            value,
        } => {
            extract_closures_from_expr_llvm(object, out, counter);
            extract_closures_from_expr_llvm(index, out, counter);
            extract_closures_from_expr_llvm(value, out, counter);
        }
        Expr::OkExpr(v)
        | Expr::ErrExpr(v)
        | Expr::SomeExpr(v)
        | Expr::Await(v)
        | Expr::Spawn(v)
        | Expr::Try(v) => {
            extract_closures_from_expr_llvm(v, out, counter);
        }
        Expr::NullCoalesce { value, default } => {
            extract_closures_from_expr_llvm(value, out, counter);
            extract_closures_from_expr_llvm(default, out, counter);
        }
        Expr::OptionalChain { object, .. } => {
            extract_closures_from_expr_llvm(object, out, counter);
        }
        Expr::Interpolation(parts) => {
            for part in parts {
                if let InterpolPart::Expr(e) = part {
                    extract_closures_from_expr_llvm(e, out, counter);
                }
            }
        }
        Expr::ArrayLit(elems) => {
            for e in elems {
                extract_closures_from_expr_llvm(e, out, counter);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, e) in fields {
                extract_closures_from_expr_llvm(e, out, counter);
            }
        }
        Expr::Match { subject, arms } => {
            extract_closures_from_expr_llvm(subject, out, counter);
            for arm in arms {
                if let Some(ref g) = arm.guard {
                    extract_closures_from_expr_llvm(g, out, counter);
                }
                extract_closures_from_expr_llvm(&arm.body, out, counter);
            }
        }
        _ => {}
    }
}

fn extract_all_closures_llvm(ast_module: &turbo_ast::Module) -> Vec<ExtractedClosure<'_>> {
    let mut closures = Vec::new();
    let mut counter = 0;
    for item in &ast_module.items {
        match &item.node {
            Item::Function(f) => {
                extract_closures_from_expr_llvm(&f.body, &mut closures, &mut counter)
            }
            Item::Impl(imp) => {
                for method in &imp.methods {
                    extract_closures_from_expr_llvm(&method.node.body, &mut closures, &mut counter);
                }
            }
            _ => {}
        }
    }
    closures
}

// ── Spawn extraction ────────────────────────────────────────────────

struct SpawnSite {
    span_start: usize,
    thunk_name: String,
    callee_name: String,
    num_args: usize,
}

fn extract_spawn_sites_from_expr_llvm(
    expr: &Spanned<Expr>,
    out: &mut Vec<SpawnSite>,
    counter: &mut usize,
) {
    match &expr.node {
        Expr::Spawn(inner) => {
            if let Expr::Call { callee, args } = &inner.node {
                if let Expr::Ident(name) = &callee.node {
                    out.push(SpawnSite {
                        span_start: expr.span.start,
                        thunk_name: format!("__spawn_thunk_{}", *counter),
                        callee_name: name.clone(),
                        num_args: args.len(),
                    });
                    *counter += 1;
                    for arg in args {
                        extract_spawn_sites_from_expr_llvm(arg, out, counter);
                    }
                    return;
                }
            }
            extract_spawn_sites_from_expr_llvm(inner, out, counter);
        }
        Expr::Block { stmts, tail_expr } => {
            for stmt in stmts {
                match &stmt.node {
                    Stmt::Let { value, .. }
                    | Stmt::LetDestructure { value, .. }
                    | Stmt::Expr(value) => {
                        extract_spawn_sites_from_expr_llvm(value, out, counter);
                    }
                    Stmt::Return(Some(e)) | Stmt::Defer(e) => {
                        extract_spawn_sites_from_expr_llvm(e, out, counter);
                    }
                    _ => {}
                }
            }
            if let Some(tail) = tail_expr {
                extract_spawn_sites_from_expr_llvm(tail, out, counter);
            }
        }
        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => {
            extract_spawn_sites_from_expr_llvm(condition, out, counter);
            extract_spawn_sites_from_expr_llvm(then_branch, out, counter);
            if let Some(e) = else_branch {
                extract_spawn_sites_from_expr_llvm(e, out, counter);
            }
        }
        Expr::While { condition, body }
        | Expr::ForIn {
            iterable: condition,
            body,
            ..
        } => {
            extract_spawn_sites_from_expr_llvm(condition, out, counter);
            extract_spawn_sites_from_expr_llvm(body, out, counter);
        }
        Expr::BinaryOp { left, right, .. } => {
            extract_spawn_sites_from_expr_llvm(left, out, counter);
            extract_spawn_sites_from_expr_llvm(right, out, counter);
        }
        Expr::UnaryOp { expr: e, .. } => extract_spawn_sites_from_expr_llvm(e, out, counter),
        Expr::Call { callee, args } => {
            extract_spawn_sites_from_expr_llvm(callee, out, counter);
            for arg in args {
                extract_spawn_sites_from_expr_llvm(arg, out, counter);
            }
        }
        Expr::Assign { value, .. } | Expr::CompoundAssign { value, .. } => {
            extract_spawn_sites_from_expr_llvm(value, out, counter);
        }
        Expr::OkExpr(v) | Expr::ErrExpr(v) | Expr::SomeExpr(v) | Expr::Await(v) | Expr::Try(v) => {
            extract_spawn_sites_from_expr_llvm(v, out, counter);
        }
        Expr::NullCoalesce { value, default } => {
            extract_spawn_sites_from_expr_llvm(value, out, counter);
            extract_spawn_sites_from_expr_llvm(default, out, counter);
        }
        Expr::OptionalChain { object, .. } => {
            extract_spawn_sites_from_expr_llvm(object, out, counter);
        }
        Expr::ArrayLit(elems) => {
            for e in elems {
                extract_spawn_sites_from_expr_llvm(e, out, counter);
            }
        }
        Expr::Match { subject, arms } => {
            extract_spawn_sites_from_expr_llvm(subject, out, counter);
            for arm in arms {
                extract_spawn_sites_from_expr_llvm(&arm.body, out, counter);
            }
        }
        _ => {}
    }
}

fn extract_all_spawn_sites_llvm(ast_module: &turbo_ast::Module) -> Vec<SpawnSite> {
    let mut sites = Vec::new();
    let mut counter = 0;
    for item in &ast_module.items {
        match &item.node {
            Item::Function(f) => {
                extract_spawn_sites_from_expr_llvm(&f.body, &mut sites, &mut counter)
            }
            Item::Impl(imp) => {
                for method in &imp.methods {
                    extract_spawn_sites_from_expr_llvm(&method.node.body, &mut sites, &mut counter);
                }
            }
            _ => {}
        }
    }
    sites
}

// ── Public entry point ──────────────────────────────────────────────

pub fn aot_compile(ast_module: &turbo_ast::Module, output_path: &Path) -> Result<(), CodegenError> {
    let context = Context::create();
    let module = context.create_module("turbo_module");
    let builder = context.create_builder();

    // Initialize target
    Target::initialize_all(&InitializationConfig::default());

    let target_triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&target_triple).map_err(|e| CodegenError {
        code: ErrorCode::E0405,
        message: format!("failed to get target: {}", e.to_string_lossy()),
    })?;
    let target_machine = target
        .create_target_machine(
            &target_triple,
            "generic",
            "",
            OptimizationLevel::Aggressive,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| CodegenError {
            code: ErrorCode::E0405,
            message: "failed to create target machine".to_string(),
        })?;

    module.set_triple(&target_triple);
    module.set_data_layout(&target_machine.get_target_data().get_data_layout());

    compile_module(&context, &module, &builder, ast_module)?;

    // Run optimization passes
    module
        .run_passes("default<O2>", &target_machine, PassBuilderOptions::create())
        .map_err(|e| CodegenError {
            code: ErrorCode::E0405,
            message: format!("LLVM optimization failed: {}", e.to_string_lossy()),
        })?;

    // Emit object file
    let tmp_dir = std::env::temp_dir().join(format!("turbo_llvm_aot_{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir).map_err(|e| CodegenError {
        code: ErrorCode::E0404,
        message: format!("failed to create temp dir: {e}"),
    })?;

    let obj_path = tmp_dir.join("turbo.o");
    let rt_path = tmp_dir.join("turbo_rt.c");

    target_machine
        .write_to_file(&module, FileType::Object, &obj_path)
        .map_err(|e| CodegenError {
            code: ErrorCode::E0404,
            message: format!("failed to emit object: {}", e.to_string_lossy()),
        })?;

    std::fs::write(&rt_path, RUNTIME_C).map_err(|e| CodegenError {
        code: ErrorCode::E0400,
        message: format!("failed to write runtime: {e}"),
    })?;

    // Link with cc
    let output = std::process::Command::new("cc")
        .arg(&rt_path)
        .arg(&obj_path)
        .arg("-lm")
        .arg("-o")
        .arg(output_path)
        .output()
        .map_err(|e| CodegenError {
            code: ErrorCode::E0404,
            message: format!("failed to run linker: {e}"),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CodegenError {
            code: ErrorCode::E0404,
            message: format!("linker failed: {stderr}"),
        });
    }

    let _ = std::fs::remove_dir_all(&tmp_dir);

    Ok(())
}

// ── Module compilation ──────────────────────────────────────────────

fn compile_module<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    builder: &Builder<'ctx>,
    ast_module: &turbo_ast::Module,
) -> Result<(), CodegenError> {
    let ptr_type = context.ptr_type(AddressSpace::default());
    let i64_type = context.i64_type();
    let i8_type = context.i8_type();
    let f64_type = context.f64_type();
    let void_type = context.void_type();

    // ── Declare runtime functions ───────────────────────────────────

    let mut rt_fns: HashMap<String, FunctionValue<'ctx>> = HashMap::new();

    macro_rules! declare_rt {
        ($name:expr, ($($param:expr),*) -> void) => {
            let fn_type = void_type.fn_type(&[$($param.into()),*], false);
            let func = module.add_function($name, fn_type, Some(inkwell::module::Linkage::External));
            rt_fns.insert($name.to_string(), func);
        };
        ($name:expr, ($($param:expr),*) -> $ret:expr) => {
            let fn_type: FunctionType = $ret.fn_type(&[$($param.into()),*], false);
            let func = module.add_function($name, fn_type, Some(inkwell::module::Linkage::External));
            rt_fns.insert($name.to_string(), func);
        };
    }

    // Core runtime
    declare_rt!("rt_print_str", (ptr_type) -> void);
    declare_rt!("rt_print_i64", (i64_type) -> void);
    declare_rt!("rt_print_f64", (f64_type) -> void);
    declare_rt!("rt_print_bool", (i8_type) -> void);
    declare_rt!("rt_panic", (ptr_type) -> void);
    declare_rt!("rt_assert_fail", (ptr_type) -> void);
    declare_rt!("rt_assert_eq_fail", (i64_type, ptr_type, ptr_type) -> void);
    declare_rt!("rt_div_by_zero", () -> void);
    declare_rt!("rt_int_overflow", () -> void);
    declare_rt!("rt_str_concat", (ptr_type, ptr_type) -> ptr_type);
    declare_rt!("rt_str_eq", (ptr_type, ptr_type) -> i8_type);
    declare_rt!("rt_array_alloc", (i64_type) -> ptr_type);
    declare_rt!("rt_array_get", (ptr_type, i64_type) -> i64_type);
    declare_rt!("rt_array_set", (ptr_type, i64_type, i64_type) -> ptr_type);
    declare_rt!("rt_array_len", (ptr_type) -> i64_type);
    declare_rt!("rt_str_len", (ptr_type) -> i64_type);
    declare_rt!("rt_struct_alloc", (i64_type) -> ptr_type);
    declare_rt!("rt_i64_to_str", (i64_type) -> ptr_type);
    declare_rt!("rt_f64_to_str", (f64_type) -> ptr_type);
    declare_rt!("rt_bool_to_str", (i8_type) -> ptr_type);
    declare_rt!("rt_result_ok", (i64_type) -> ptr_type);
    declare_rt!("rt_result_err", (i64_type) -> ptr_type);
    declare_rt!("rt_result_tag", (ptr_type) -> i64_type);
    declare_rt!("rt_result_value", (ptr_type) -> i64_type);
    declare_rt!("rt_option_some", (i64_type) -> ptr_type);
    declare_rt!("rt_option_none", () -> ptr_type);
    declare_rt!("rt_option_tag", (ptr_type) -> i64_type);
    declare_rt!("rt_option_value", (ptr_type) -> i64_type);
    // Stdlib
    declare_rt!("rt_str_split", (ptr_type, ptr_type) -> ptr_type);
    declare_rt!("rt_str_trim", (ptr_type) -> ptr_type);
    declare_rt!("rt_str_upper", (ptr_type) -> ptr_type);
    declare_rt!("rt_str_lower", (ptr_type) -> ptr_type);
    declare_rt!("rt_str_starts_with", (ptr_type, ptr_type) -> i8_type);
    declare_rt!("rt_str_ends_with", (ptr_type, ptr_type) -> i8_type);
    declare_rt!("rt_str_replace", (ptr_type, ptr_type, ptr_type) -> ptr_type);
    declare_rt!("rt_str_char_at", (ptr_type, i64_type) -> ptr_type);
    declare_rt!("rt_str_contains", (ptr_type, ptr_type) -> i8_type);
    declare_rt!("rt_str_index_of", (ptr_type, ptr_type) -> i64_type);
    declare_rt!("rt_str_join", (ptr_type, ptr_type) -> ptr_type);
    declare_rt!("rt_str_repeat", (ptr_type, i64_type) -> ptr_type);
    declare_rt!("rt_read_line", () -> ptr_type);
    declare_rt!("rt_read_file", (ptr_type) -> ptr_type);
    declare_rt!("rt_write_file", (ptr_type, ptr_type) -> void);
    declare_rt!("rt_pow", (i64_type, i64_type) -> i64_type);
    declare_rt!("rt_sqrt", (f64_type) -> f64_type);
    // Async
    declare_rt!("rt_sleep_ms", (i64_type) -> void);
    declare_rt!("rt_spawn_with_args", (ptr_type, ptr_type) -> ptr_type);
    declare_rt!("rt_await_handle", (ptr_type) -> i64_type);
    // HTTP + JSON
    declare_rt!("rt_http_get", (ptr_type) -> ptr_type);
    declare_rt!("rt_http_post", (ptr_type, ptr_type) -> ptr_type);
    declare_rt!("rt_json_get", (ptr_type, ptr_type) -> ptr_type);
    declare_rt!("rt_json_stringify", (ptr_type, ptr_type) -> ptr_type);
    // HTTP server
    declare_rt!("rt_http_server", (i64_type) -> i64_type);
    declare_rt!("rt_http_route", (i64_type, ptr_type, ptr_type, ptr_type, ptr_type) -> void);
    declare_rt!("rt_http_listen", (i64_type) -> void);
    declare_rt!("rt_respond", (i64_type, ptr_type) -> ptr_type);
    declare_rt!("rt_request_body", (ptr_type) -> ptr_type);
    // Channels
    declare_rt!("rt_channel_create", () -> ptr_type);
    declare_rt!("rt_channel_send", (ptr_type, i64_type) -> void);
    declare_rt!("rt_channel_recv", (ptr_type) -> i64_type);
    declare_rt!("rt_channel_clone_sender", (ptr_type) -> ptr_type);
    // Mutex
    declare_rt!("rt_mutex_create", (i64_type) -> ptr_type);
    declare_rt!("rt_mutex_get", (ptr_type) -> i64_type);
    declare_rt!("rt_mutex_set", (ptr_type, i64_type) -> void);
    declare_rt!("rt_mutex_clone", (ptr_type) -> ptr_type);
    // HashMap
    declare_rt!("rt_hashmap_new", () -> ptr_type);
    declare_rt!("rt_hashmap_set", (ptr_type, ptr_type, ptr_type) -> void);
    declare_rt!("rt_hashmap_get", (ptr_type, ptr_type) -> ptr_type);
    declare_rt!("rt_hashmap_has", (ptr_type, ptr_type) -> i8_type);
    declare_rt!("rt_hashmap_len", (ptr_type) -> i64_type);
    declare_rt!("rt_hashmap_keys", (ptr_type) -> ptr_type);
    declare_rt!("rt_hashmap_remove", (ptr_type, ptr_type) -> void);
    // ARC
    declare_rt!("rt_retain", (ptr_type) -> void);
    declare_rt!("rt_release", (ptr_type) -> void);

    // ── Build enum/struct metadata ──────────────────────────────────

    let mut enum_variants: HashMap<String, Vec<String>> = HashMap::new();
    let mut enum_variant_fields: HashMap<(String, String), Vec<TurboTy>> = HashMap::new();
    let mut enum_max_slots: HashMap<String, usize> = HashMap::new();
    for item in &ast_module.items {
        if let Item::Enum(e) = &item.node {
            let variant_names: Vec<String> = e.variants.iter().map(|v| v.name.clone()).collect();
            let tp_names = e.type_param_names();
            let mut max_fields: usize = 0;
            for v in &e.variants {
                let field_tys: Vec<TurboTy> = v
                    .fields
                    .iter()
                    .map(|f| {
                        turbo_ty_from_type_expr_with_params(&f.node, &enum_variants, &tp_names)
                    })
                    .collect();
                if !field_tys.is_empty() {
                    max_fields = max_fields.max(field_tys.len());
                    enum_variant_fields.insert((e.name.clone(), v.name.clone()), field_tys);
                }
            }
            if max_fields > 0 {
                enum_max_slots.insert(e.name.clone(), max_fields);
            }
            enum_variants.insert(e.name.clone(), variant_names);
        }
    }

    let mut struct_fields: HashMap<String, Vec<(String, TurboTy)>> = HashMap::new();
    for item in &ast_module.items {
        if let Item::Struct(s) = &item.node {
            let tp_names = s.type_param_names();
            let fields: Vec<(String, TurboTy)> = s
                .fields
                .iter()
                .map(|f| {
                    (
                        f.name.clone(),
                        turbo_ty_from_type_expr_with_params(&f.ty.node, &enum_variants, &tp_names),
                    )
                })
                .collect();
            struct_fields.insert(s.name.clone(), fields);
        }
    }

    // Build constants map
    let mut constants_map: HashMap<String, Spanned<Expr>> = HashMap::new();
    for item in &ast_module.items {
        if let Item::Const(c) = &item.node {
            constants_map.insert(c.name.clone(), c.value.clone());
        }
    }

    // ── Build struct derives map ────────────────────────────────────
    let mut struct_derives: HashMap<String, Vec<String>> = HashMap::new();
    for item in &ast_module.items {
        if let Item::Struct(s) = &item.node {
            if !s.derives.is_empty() {
                struct_derives.insert(s.name.clone(), s.derives.clone());
            }
        }
    }

    // ── Build trait impls map ───────────────────────────────────────
    let mut trait_impls: HashMap<String, Vec<String>> = HashMap::new();
    for item in &ast_module.items {
        if let Item::Impl(imp) = &item.node {
            if let Some(ref trait_name) = imp.trait_name {
                trait_impls
                    .entry(imp.type_name.clone())
                    .or_default()
                    .push(trait_name.clone());
            }
        }
    }
    // @derive(Display) counts as implementing Display
    for (sname, derives) in &struct_derives {
        if derives.contains(&"Display".to_string()) {
            trait_impls
                .entry(sname.clone())
                .or_default()
                .push("Display".to_string());
        }
    }

    // ── Build agent struct info ─────────────────────────────────────
    // Register agent names as structs so StructLit works for agent instantiation
    let mut agent_names = std::collections::HashSet::new();
    let mut agent_defs: HashMap<String, (String, Vec<String>, Option<String>)> = HashMap::new();
    for item in &ast_module.items {
        if let Item::Agent(agent) = &item.node {
            agent_names.insert(agent.name.clone());
            agent_defs.insert(
                agent.name.clone(),
                (
                    agent.model.clone(),
                    agent.tools.clone(),
                    agent.system_prompt.clone(),
                ),
            );
            if !struct_fields.contains_key(&agent.name) {
                struct_fields.insert(
                    agent.name.clone(),
                    vec![
                        ("model".to_string(), TurboTy::Str),
                        ("system".to_string(), TurboTy::Str),
                        ("tools".to_string(), TurboTy::Array(Box::new(TurboTy::Str))),
                    ],
                );
            }
        }
    }

    // ── Extract closures and spawn sites ───────────────────────────
    let all_closures = extract_all_closures_llvm(ast_module);
    let all_spawn_sites = extract_all_spawn_sites_llvm(ast_module);

    // Build lookup maps
    let mut closure_fns: HashMap<usize, (String, TurboTy, Vec<String>)> = HashMap::new();
    let mut spawn_thunks_map: HashMap<usize, String> = HashMap::new();

    for site in &all_spawn_sites {
        spawn_thunks_map.insert(site.span_start, site.thunk_name.clone());
    }

    // ── Declare all user functions ──────────────────────────────────

    let mut user_fns: HashMap<String, FunctionValue<'ctx>> = HashMap::new();
    let mut fn_ret_types: HashMap<String, TurboTy> = HashMap::new();
    let mut fn_asts: HashMap<String, &FnDef> = HashMap::new();
    let mut fn_type_params: HashMap<String, Vec<String>> = HashMap::new();

    for item in &ast_module.items {
        let Item::Function(f) = &item.node else {
            continue;
        };

        let tp_names = f.type_param_names();
        let param_types: Vec<BasicMetadataTypeEnum<'ctx>> = f
            .params
            .iter()
            .map(|p| {
                resolve_llvm_type_ctx(
                    &p.ty.node,
                    context,
                    &enum_variants,
                    &enum_max_slots,
                    &tp_names,
                )
                .into()
            })
            .collect();

        let ret_turbo = if let Some(ret_ty) = &f.return_type {
            turbo_ty_from_type_expr_with_params(&ret_ty.node, &enum_variants, &tp_names)
        } else {
            TurboTy::Unit
        };

        let fn_type = if ret_turbo == TurboTy::Unit {
            void_type.fn_type(&param_types, false)
        } else {
            let ret_llvm = turbo_ty_to_llvm_ctx(&ret_turbo, context, &enum_max_slots);
            ret_llvm.fn_type(&param_types, false)
        };

        // For AOT, rename main -> turbo_main (the C runtime provides the real main)
        let sym_name = if f.name == "main" {
            "turbo_main"
        } else {
            &f.name
        };

        let func = module.add_function(sym_name, fn_type, None);
        user_fns.insert(f.name.clone(), func);
        fn_ret_types.insert(f.name.clone(), ret_turbo);
        fn_asts.insert(f.name.clone(), f);
        fn_type_params.insert(f.name.clone(), tp_names);
    }

    // Declare methods from impl blocks (including trait impls + default trait methods)
    for item in &ast_module.items {
        let Item::Impl(imp) = &item.node else {
            continue;
        };
        for method_spanned in &imp.methods {
            let method = &method_spanned.node;
            let mangled = format!("{}__{}", imp.type_name, method.name);

            let param_types: Vec<BasicMetadataTypeEnum<'ctx>> = method
                .params
                .iter()
                .map(|p| {
                    if p.name == "self" {
                        ptr_type.into()
                    } else {
                        resolve_llvm_type_ctx(
                            &p.ty.node,
                            context,
                            &enum_variants,
                            &enum_max_slots,
                            &[],
                        )
                        .into()
                    }
                })
                .collect();

            let ret_turbo = if let Some(ret_ty) = &method.return_type {
                turbo_ty_from_type_expr(&ret_ty.node, &enum_variants)
            } else {
                TurboTy::Unit
            };

            let fn_type = if ret_turbo == TurboTy::Unit {
                void_type.fn_type(&param_types, false)
            } else {
                let ret_llvm = turbo_ty_to_llvm_ctx(&ret_turbo, context, &enum_max_slots);
                ret_llvm.fn_type(&param_types, false)
            };

            let func = module.add_function(&mangled, fn_type, None);
            user_fns.insert(mangled.clone(), func);
            fn_ret_types.insert(mangled, ret_turbo);
        }
    }

    // Declare default trait methods (methods defined in trait body that aren't overridden by impl)
    let mut trait_defs: HashMap<String, &turbo_ast::TraitDef> = HashMap::new();
    for item in &ast_module.items {
        if let Item::Trait(t) = &item.node {
            trait_defs.insert(t.name.clone(), t);
        }
    }
    for item in &ast_module.items {
        if let Item::Impl(imp) = &item.node {
            let trait_name = match &imp.trait_name {
                Some(t) => t.clone(),
                None => continue,
            };
            let trait_def = match trait_defs.get(&trait_name) {
                Some(t) => *t,
                None => continue,
            };
            let implemented: std::collections::HashSet<String> =
                imp.methods.iter().map(|m| m.node.name.clone()).collect();
            for trait_method in &trait_def.methods {
                if implemented.contains(&trait_method.name) {
                    continue;
                }
                let body_expr = match &trait_method.default_body {
                    Some(b) => b,
                    None => continue,
                };
                let mangled = format!("{}__{}", imp.type_name, trait_method.name);
                let param_types: Vec<BasicMetadataTypeEnum<'ctx>> = trait_method
                    .params
                    .iter()
                    .map(|p| {
                        if p.name == "self" {
                            ptr_type.into()
                        } else {
                            resolve_llvm_type_ctx(
                                &p.ty.node,
                                context,
                                &enum_variants,
                                &enum_max_slots,
                                &[],
                            )
                            .into()
                        }
                    })
                    .collect();
                let ret_turbo = if let Some(ref rt) = trait_method.return_type {
                    turbo_ty_from_type_expr(&rt.node, &enum_variants)
                } else {
                    TurboTy::Unit
                };
                let fn_llvm = if ret_turbo == TurboTy::Unit {
                    void_type.fn_type(&param_types, false)
                } else {
                    let rl = turbo_ty_to_llvm_ctx(&ret_turbo, context, &enum_max_slots);
                    rl.fn_type(&param_types, false)
                };
                let func = module.add_function(&mangled, fn_llvm, None);
                user_fns.insert(mangled.clone(), func);
                fn_ret_types.insert(mangled.clone(), ret_turbo);
                // Store the body for compilation later; we use fn_asts to point to a trait method
                // We'll compile the body inline below using fn_asts for the type_name context
                let _ = body_expr; // will compile body in "define bodies" loop
            }
        }
    }

    // Declare derived methods (Display to_string, Eq ==, Clone clone)
    for (struct_name, derives) in &struct_derives {
        for derive_name in derives {
            match derive_name.as_str() {
                "Display" => {
                    let mangled = format!("{struct_name}__to_string");
                    if !user_fns.contains_key(&mangled) {
                        let fn_type = ptr_type.fn_type(&[ptr_type.into()], false);
                        let func = module.add_function(&mangled, fn_type, None);
                        user_fns.insert(mangled.clone(), func);
                        fn_ret_types.insert(mangled, TurboTy::Str);
                    }
                }
                "Eq" => {
                    let mangled = format!("{struct_name}__eq");
                    if !user_fns.contains_key(&mangled) {
                        let fn_type = i8_type.fn_type(&[ptr_type.into(), ptr_type.into()], false);
                        let func = module.add_function(&mangled, fn_type, None);
                        user_fns.insert(mangled.clone(), func);
                        fn_ret_types.insert(mangled, TurboTy::Bool);
                    }
                }
                "Clone" => {
                    let mangled = format!("{struct_name}__clone");
                    if !user_fns.contains_key(&mangled) {
                        let fn_type = ptr_type.fn_type(&[ptr_type.into()], false);
                        let func = module.add_function(&mangled, fn_type, None);
                        user_fns.insert(mangled.clone(), func);
                        fn_ret_types.insert(mangled, TurboTy::Struct(struct_name.clone()));
                    }
                }
                _ => {}
            }
        }
    }

    // Declare pre-extracted closures
    for cl in &all_closures {
        let mut param_tys: Vec<BasicMetadataTypeEnum<'ctx>> = vec![ptr_type.into()]; // env_ptr first
        let mut turbo_param_tys: Vec<TurboTy> = Vec::new();
        for p in cl.params {
            let tty = turbo_ty_from_type_expr(&p.ty.node, &enum_variants);
            param_tys.push(turbo_ty_to_llvm(&tty, context).into());
            turbo_param_tys.push(tty);
        }
        let ret_turbo = if let Some(ref rt) = cl.return_type {
            turbo_ty_from_type_expr(&rt.node, &enum_variants)
        } else {
            TurboTy::Int
        };
        let fn_llvm = if ret_turbo == TurboTy::Unit {
            void_type.fn_type(&param_tys, false)
        } else {
            let rl = turbo_ty_to_llvm(&ret_turbo, context);
            rl.fn_type(&param_tys, false)
        };
        let func = module.add_function(&cl.name, fn_llvm, None);
        user_fns.insert(cl.name.clone(), func);
        let closure_turbo_ty = TurboTy::Fn(turbo_param_tys, Box::new(ret_turbo));
        fn_ret_types.insert(
            cl.name.clone(),
            match &closure_turbo_ty {
                TurboTy::Fn(_, r) => *r.clone(),
                _ => TurboTy::Int,
            },
        );
        closure_fns.insert(
            cl.span_start,
            (cl.name.clone(), closure_turbo_ty, cl.free_vars.clone()),
        );
    }

    // Declare spawn thunks
    for site in &all_spawn_sites {
        // Thunk signature: fn(__spawn_thunk_N)(args_ptr: *) -> i64
        let fn_type = i64_type.fn_type(&[ptr_type.into()], false);
        let func = module.add_function(&site.thunk_name, fn_type, None);
        user_fns.insert(site.thunk_name.clone(), func);
        fn_ret_types.insert(site.thunk_name.clone(), TurboTy::Unit);
    }

    // ── Define all function bodies ──────────────────────────────────

    let mut string_counter: usize = 0;

    for item in &ast_module.items {
        let Item::Function(f) = &item.node else {
            continue;
        };
        let func = user_fns[&f.name];
        let tp_names = f.type_param_names();

        let entry = context.append_basic_block(func, "entry");
        builder.position_at_end(entry);

        let mut vars: HashMap<String, (PointerValue<'ctx>, TurboTy)> = HashMap::new();

        // Create allocas for parameters
        for (i, param) in f.params.iter().enumerate() {
            let llvm_ty = resolve_llvm_type_ctx(
                &param.ty.node,
                context,
                &enum_variants,
                &enum_max_slots,
                &tp_names,
            );
            let turbo_ty =
                turbo_ty_from_type_expr_with_params(&param.ty.node, &enum_variants, &tp_names);
            let alloca = builder
                .build_alloca(llvm_ty, &param.name)
                .expect("build_alloca failed");
            let param_val = func.get_nth_param(i as u32).unwrap();
            builder
                .build_store(alloca, param_val)
                .expect("build_store failed");
            vars.insert(param.name.clone(), (alloca, turbo_ty));
        }

        macro_rules! make_ctx {
            ($vars:expr, $current_fn:expr) => {
                Ctx {
                    context,
                    module,
                    builder,
                    current_fn: $current_fn,
                    user_fns: &user_fns,
                    fn_ret_types: &fn_ret_types,
                    fn_asts: &fn_asts,
                    fn_type_params: &fn_type_params,
                    rt_fns: &rt_fns,
                    vars: $vars,
                    string_counter: &mut string_counter,
                    struct_fields: &struct_fields,
                    enum_variants: &enum_variants,
                    enum_variant_fields: &enum_variant_fields,
                    enum_max_slots: &enum_max_slots,
                    constants: &constants_map,
                    loop_stack: Vec::new(),
                    closure_fns: &closure_fns,
                    spawn_thunks: &spawn_thunks_map,
                    struct_derives: &struct_derives,
                    trait_impls: &trait_impls,
                    agent_names: &agent_names,
                    agent_defs: &agent_defs,
                    concrete_struct_fields: std::collections::HashMap::new(),
                }
            };
        }

        let mut cx = make_ctx!(vars, func);

        let result = compile_expr(&mut cx, &f.body)?;

        // Only emit a terminator if the current block doesn't have one
        let current_block = builder.get_insert_block().unwrap();
        if current_block.get_terminator().is_none() {
            if f.return_type.is_some() {
                if let Some((val, _)) = result {
                    builder
                        .build_return(Some(&val))
                        .expect("build_return failed");
                } else {
                    builder.build_return(None).expect("build_return failed");
                }
            } else {
                builder.build_return(None).expect("build_return failed");
            }
        }
    }

    macro_rules! make_ctx_global {
        ($vars:expr, $current_fn:expr) => {
            Ctx {
                context,
                module,
                builder,
                current_fn: $current_fn,
                user_fns: &user_fns,
                fn_ret_types: &fn_ret_types,
                fn_asts: &fn_asts,
                fn_type_params: &fn_type_params,
                rt_fns: &rt_fns,
                vars: $vars,
                string_counter: &mut string_counter,
                struct_fields: &struct_fields,
                enum_variants: &enum_variants,
                enum_variant_fields: &enum_variant_fields,
                enum_max_slots: &enum_max_slots,
                constants: &constants_map,
                loop_stack: Vec::new(),
                closure_fns: &closure_fns,
                spawn_thunks: &spawn_thunks_map,
                struct_derives: &struct_derives,
                trait_impls: &trait_impls,
                agent_names: &agent_names,
                agent_defs: &agent_defs,
                concrete_struct_fields: HashMap::new(),
            }
        };
    }

    // Define method bodies from impl blocks
    for item in &ast_module.items {
        let Item::Impl(imp) = &item.node else {
            continue;
        };
        for method_spanned in &imp.methods {
            let method = &method_spanned.node;
            let mangled = format!("{}__{}", imp.type_name, method.name);
            let func = user_fns[&mangled];

            let entry = context.append_basic_block(func, "entry");
            builder.position_at_end(entry);

            let mut vars: HashMap<String, (PointerValue<'ctx>, TurboTy)> = HashMap::new();

            for (i, param) in method.params.iter().enumerate() {
                let (llvm_ty, turbo_ty) = if param.name == "self" {
                    (
                        ptr_type.as_basic_type_enum(),
                        TurboTy::Struct(imp.type_name.clone()),
                    )
                } else {
                    let llvm_ty = resolve_llvm_type_ctx(
                        &param.ty.node,
                        context,
                        &enum_variants,
                        &enum_max_slots,
                        &[],
                    );
                    let turbo_ty = turbo_ty_from_type_expr(&param.ty.node, &enum_variants);
                    (llvm_ty, turbo_ty)
                };
                let alloca = builder
                    .build_alloca(llvm_ty, &param.name)
                    .expect("build_alloca failed");
                let param_val = func.get_nth_param(i as u32).unwrap();
                builder
                    .build_store(alloca, param_val)
                    .expect("build_store failed");
                vars.insert(param.name.clone(), (alloca, turbo_ty));
            }

            let mut cx = make_ctx_global!(vars, func);

            let result = compile_expr(&mut cx, &method.body)?;

            let current_block = builder.get_insert_block().unwrap();
            if current_block.get_terminator().is_none() {
                if method.return_type.is_some() {
                    if let Some((val, _)) = result {
                        builder
                            .build_return(Some(&val))
                            .expect("build_return failed");
                    } else {
                        builder.build_return(None).expect("build_return failed");
                    }
                } else {
                    builder.build_return(None).expect("build_return failed");
                }
            }
        }
    }

    // Define default trait method bodies
    for item in &ast_module.items {
        if let Item::Impl(imp) = &item.node {
            let trait_name = match &imp.trait_name {
                Some(t) => t.clone(),
                None => continue,
            };
            let trait_def = match trait_defs.get(&trait_name) {
                Some(t) => *t,
                None => continue,
            };
            let implemented: std::collections::HashSet<String> =
                imp.methods.iter().map(|m| m.node.name.clone()).collect();
            for trait_method in &trait_def.methods {
                if implemented.contains(&trait_method.name) {
                    continue;
                }
                let body_expr = match &trait_method.default_body {
                    Some(b) => b,
                    None => continue,
                };
                let mangled = format!("{}__{}", imp.type_name, trait_method.name);
                let func = match user_fns.get(&mangled) {
                    Some(f) => *f,
                    None => continue,
                };

                let entry = context.append_basic_block(func, "entry");
                builder.position_at_end(entry);

                let mut vars: HashMap<String, (PointerValue<'ctx>, TurboTy)> = HashMap::new();
                for (i, param) in trait_method.params.iter().enumerate() {
                    let (llvm_ty, turbo_ty) = if param.name == "self" {
                        (
                            ptr_type.as_basic_type_enum(),
                            TurboTy::Struct(imp.type_name.clone()),
                        )
                    } else {
                        let lt = resolve_llvm_type_ctx(
                            &param.ty.node,
                            context,
                            &enum_variants,
                            &enum_max_slots,
                            &[],
                        );
                        let tt = turbo_ty_from_type_expr(&param.ty.node, &enum_variants);
                        (lt, tt)
                    };
                    let alloca = builder
                        .build_alloca(llvm_ty, &param.name)
                        .expect("alloca failed");
                    builder
                        .build_store(alloca, func.get_nth_param(i as u32).unwrap())
                        .expect("store failed");
                    vars.insert(param.name.clone(), (alloca, turbo_ty));
                }

                let mut cx = make_ctx_global!(vars, func);
                let result = compile_expr(&mut cx, body_expr)?;
                let cur = builder.get_insert_block().unwrap();
                if cur.get_terminator().is_none() {
                    if trait_method.return_type.is_some() {
                        if let Some((val, _)) = result {
                            builder.build_return(Some(&val)).expect("return failed");
                        } else {
                            builder.build_return(None).expect("return failed");
                        }
                    } else {
                        builder.build_return(None).expect("return failed");
                    }
                }
            }
        }
    }

    // Define derived method bodies
    for (struct_name, derives) in &struct_derives {
        let fields = struct_fields.get(struct_name).cloned().unwrap_or_default();

        for derive_name in derives {
            match derive_name.as_str() {
                "Display" => {
                    let mangled = format!("{struct_name}__to_string");
                    let func = match user_fns.get(&mangled) {
                        Some(f) => *f,
                        None => continue,
                    };
                    // Already declared. Define: concatenate all fields as "StructName { f1: v1, f2: v2 }"
                    let entry = context.append_basic_block(func, "entry");
                    builder.position_at_end(entry);
                    let vars: HashMap<String, (PointerValue<'ctx>, TurboTy)> = {
                        let mut m = HashMap::new();
                        let self_alloca = builder.build_alloca(ptr_type, "self").expect("alloca");
                        builder
                            .build_store(self_alloca, func.get_nth_param(0).unwrap())
                            .expect("store");
                        m.insert(
                            "self".to_string(),
                            (self_alloca, TurboTy::Struct(struct_name.clone())),
                        );
                        m
                    };
                    let mut cx = make_ctx_global!(vars, func);

                    // Build display string: "StructName { field1: val1, field2: val2 }"
                    let mut parts: Vec<BasicValueEnum<'ctx>> = Vec::new();
                    let prefix = cx
                        .create_string(&format!("{struct_name} {{ "))
                        .expect("str");
                    parts.push(prefix.into());

                    let self_ptr = cx
                        .builder
                        .build_load(ptr_type, cx.vars["self"].0, "self_ptr")
                        .expect("load self")
                        .into_pointer_value();

                    for (fi, (fname, ftty)) in fields.iter().enumerate() {
                        if fi > 0 {
                            let sep = cx.create_string(", ").expect("str");
                            parts.push(sep.into());
                        }
                        let flabel = cx.create_string(&format!("{fname}: ")).expect("str");
                        parts.push(flabel.into());

                        let offset = fi as u64 * 8;
                        let field_ptr = unsafe {
                            cx.builder
                                .build_gep(
                                    i8_type,
                                    self_ptr,
                                    &[i64_type.const_int(offset, false)],
                                    "fp",
                                )
                                .expect("gep")
                        };
                        let raw = cx
                            .builder
                            .build_load(i64_type, field_ptr, "fv")
                            .expect("load");
                        let fval = narrow_from_storage(&cx, raw.into(), ftty);
                        let fstr =
                            convert_to_str(&mut cx, fval, ftty).unwrap_or_else(|_| raw.into());
                        parts.push(fstr);
                    }

                    let suffix = cx.create_string(" }").expect("str");
                    parts.push(suffix.into());

                    // Concatenate all parts
                    let mut result_str: BasicValueEnum<'ctx> =
                        cx.create_string("").expect("str").into();
                    for part in parts {
                        result_str = cx
                            .rt_call("rt_str_concat", &[result_str.into(), part.into()])
                            .unwrap();
                    }
                    cx.builder.build_return(Some(&result_str)).expect("return");
                }
                "Eq" => {
                    let mangled = format!("{struct_name}__eq");
                    let func = match user_fns.get(&mangled) {
                        Some(f) => *f,
                        None => continue,
                    };
                    let entry = context.append_basic_block(func, "entry");
                    builder.position_at_end(entry);
                    // eq(self, other) -> bool: compare each field
                    let self_ptr = func.get_nth_param(0).unwrap().into_pointer_value();
                    let other_ptr = func.get_nth_param(1).unwrap().into_pointer_value();

                    // Compare all fields; return false at first mismatch
                    let merge_block = context.append_basic_block(func, "eq_merge");
                    let mut phi_sources: Vec<(
                        BasicValueEnum<'ctx>,
                        inkwell::basic_block::BasicBlock<'ctx>,
                    )> = Vec::new();

                    let mut cur_block = entry;
                    for (fi, (_, ftty)) in fields.iter().enumerate() {
                        let offset = fi as u64 * 8;
                        let fp1 = unsafe {
                            builder
                                .build_gep(
                                    i8_type,
                                    self_ptr,
                                    &[i64_type.const_int(offset, false)],
                                    "fp1",
                                )
                                .expect("gep")
                        };
                        let fp2 = unsafe {
                            builder
                                .build_gep(
                                    i8_type,
                                    other_ptr,
                                    &[i64_type.const_int(offset, false)],
                                    "fp2",
                                )
                                .expect("gep")
                        };
                        let v1 = builder
                            .build_load(i64_type, fp1, "v1")
                            .expect("load")
                            .into_int_value();
                        let v2 = builder
                            .build_load(i64_type, fp2, "v2")
                            .expect("load")
                            .into_int_value();

                        let cmp = if *ftty == TurboTy::Str {
                            let pv1 = builder.build_int_to_ptr(v1, ptr_type, "p1").expect("itp");
                            let pv2 = builder.build_int_to_ptr(v2, ptr_type, "p2").expect("itp");
                            let eq = builder
                                .build_direct_call(
                                    rt_fns["rt_str_eq"],
                                    &[pv1.into(), pv2.into()],
                                    "seq",
                                )
                                .expect("call");
                            let eq_i = eq.try_as_basic_value().left().unwrap().into_int_value();
                            let zero = i8_type.const_int(0, false);
                            builder
                                .build_int_compare(IntPredicate::NE, eq_i, zero, "cmp")
                                .expect("cmp")
                        } else {
                            builder
                                .build_int_compare(IntPredicate::EQ, v1, v2, "cmp")
                                .expect("cmp")
                        };

                        let next_block = context.append_basic_block(func, "eq_next");
                        let false_val = i8_type.const_int(0, false);
                        phi_sources.push((false_val.into(), cur_block));
                        builder
                            .build_conditional_branch(cmp, next_block, merge_block)
                            .expect("br");
                        builder.position_at_end(next_block);
                        cur_block = next_block;
                    }
                    let true_val = i8_type.const_int(1, false);
                    phi_sources.push((true_val.into(), cur_block));
                    builder.build_unconditional_branch(merge_block).expect("br");
                    builder.position_at_end(merge_block);
                    let phi = builder.build_phi(i8_type, "eq_result").expect("phi");
                    for (val, block) in &phi_sources {
                        phi.add_incoming(&[(val, *block)]);
                    }
                    builder
                        .build_return(Some(&phi.as_basic_value()))
                        .expect("return");
                }
                "Clone" => {
                    let mangled = format!("{struct_name}__clone");
                    let func = match user_fns.get(&mangled) {
                        Some(f) => *f,
                        None => continue,
                    };
                    let entry = context.append_basic_block(func, "entry");
                    builder.position_at_end(entry);
                    // clone(self) -> Self: alloc new struct, copy all fields
                    let self_ptr = func.get_nth_param(0).unwrap().into_pointer_value();
                    let nf = fields.len() as u64;
                    let new_ptr = builder
                        .build_direct_call(
                            rt_fns["rt_struct_alloc"],
                            &[i64_type.const_int(nf, false).into()],
                            "clone_ptr",
                        )
                        .expect("call")
                        .try_as_basic_value()
                        .left()
                        .unwrap()
                        .into_pointer_value();
                    for fi in 0..fields.len() {
                        let offset = fi as u64 * 8;
                        let sp = unsafe {
                            builder
                                .build_gep(
                                    i8_type,
                                    self_ptr,
                                    &[i64_type.const_int(offset, false)],
                                    "sp",
                                )
                                .expect("gep")
                        };
                        let dp = unsafe {
                            builder
                                .build_gep(
                                    i8_type,
                                    new_ptr,
                                    &[i64_type.const_int(offset, false)],
                                    "dp",
                                )
                                .expect("gep")
                        };
                        let val = builder.build_load(i64_type, sp, "val").expect("load");
                        builder.build_store(dp, val).expect("store");
                    }
                    builder
                        .build_return(Some(&BasicValueEnum::PointerValue(new_ptr)))
                        .expect("return");
                }
                _ => {}
            }
        }
    }

    // Define closure bodies
    for cl in &all_closures {
        let func = match user_fns.get(&cl.name) {
            Some(f) => *f,
            None => continue,
        };
        let entry = context.append_basic_block(func, "entry");
        builder.position_at_end(entry);

        let mut vars: HashMap<String, (PointerValue<'ctx>, TurboTy)> = HashMap::new();

        // First param is env_ptr
        let env_ptr_val = func.get_nth_param(0).unwrap().into_pointer_value();

        // Closure params start at index 1
        for (i, param) in cl.params.iter().enumerate() {
            let tty = turbo_ty_from_type_expr(&param.ty.node, &enum_variants);
            let llvm_ty = turbo_ty_to_llvm(&tty, context);
            let alloca = builder.build_alloca(llvm_ty, &param.name).expect("alloca");
            let param_val = func.get_nth_param((i + 1) as u32).unwrap();
            builder.build_store(alloca, param_val).expect("store");
            vars.insert(param.name.clone(), (alloca, tty));
        }

        // Load captured variables from env_ptr
        // We need to find what was captured — we'll populate during compile by scanning free_vars
        // For now, load each captured var from env struct at compile time
        // The free_vars list tells us which outer vars are captured
        for (cap_idx, cap_name) in cl.free_vars.iter().enumerate() {
            let cap_tty = cl
                .capture_types
                .get(cap_idx)
                .cloned()
                .unwrap_or(TurboTy::Int);
            let offset = cap_idx as u64 * 8;
            let field_ptr = unsafe {
                builder
                    .build_gep(
                        i8_type,
                        env_ptr_val,
                        &[i64_type.const_int(offset, false)],
                        "cap_ptr",
                    )
                    .expect("gep")
            };
            let raw = builder
                .build_load(i64_type, field_ptr, cap_name)
                .expect("load");
            // If the captured variable is a string (ptr), convert i64 → ptr
            if matches!(cap_tty, TurboTy::Str | TurboTy::Array(_)) {
                let ptr_val = builder
                    .build_int_to_ptr(
                        raw.into_int_value(),
                        ptr_type,
                        &format!("cap_ptr_{cap_name}"),
                    )
                    .expect("itp");
                let alloca = builder
                    .build_alloca(ptr_type, &format!("cap_{cap_name}"))
                    .expect("alloca");
                builder.build_store(alloca, ptr_val).expect("store");
                vars.insert(cap_name.clone(), (alloca, cap_tty));
            } else {
                let alloca = builder
                    .build_alloca(i64_type, &format!("cap_{cap_name}"))
                    .expect("alloca");
                builder.build_store(alloca, raw).expect("store");
                vars.insert(cap_name.clone(), (alloca, cap_tty));
            }
        }

        let mut cx = make_ctx_global!(vars, func);
        let result = compile_expr(&mut cx, cl.body)?;
        let cur = builder.get_insert_block().unwrap();
        if cur.get_terminator().is_none() {
            let ret_turbo = if let Some(ref rt) = cl.return_type {
                turbo_ty_from_type_expr(&rt.node, &enum_variants)
            } else {
                TurboTy::Int
            }; // Default to Int to match function declaration
            if ret_turbo != TurboTy::Unit {
                if let Some((val, _)) = result {
                    builder.build_return(Some(&val)).expect("return");
                } else {
                    // No value from body — return a zero/null of the expected type
                    let dummy: BasicValueEnum = match &ret_turbo {
                        TurboTy::Int => i64_type.const_int(0, false).into(),
                        TurboTy::Bool => i8_type.const_int(0, false).into(),
                        TurboTy::Float => context.f64_type().const_float(0.0).into(),
                        _ => ptr_type.const_null().into(),
                    };
                    builder.build_return(Some(&dummy)).expect("return");
                }
            } else {
                builder.build_return(None).expect("return");
            }
        }
    }

    // Define spawn thunk bodies
    for site in &all_spawn_sites {
        let func = match user_fns.get(&site.thunk_name) {
            Some(f) => *f,
            None => continue,
        };
        let entry = context.append_basic_block(func, "entry");
        builder.position_at_end(entry);
        // args_ptr points to [fn_ptr, arg0, arg1, ...]
        let args_ptr = func.get_nth_param(0).unwrap().into_pointer_value();

        // Load fn_ptr from offset 0
        let fn_ptr_i64 = builder
            .build_load(i64_type, args_ptr, "fn_ptr_i64")
            .expect("load");
        let fn_ptr = builder
            .build_int_to_ptr(fn_ptr_i64.into_int_value(), ptr_type, "fn_ptr")
            .expect("itp");

        // Load each arg from offsets 8, 16, ...
        let target_fn = user_fns.get(&site.callee_name);
        let mut arg_vals: Vec<BasicMetadataValueEnum<'ctx>> = Vec::new();
        for i in 0..site.num_args {
            let offset = (i + 1) as u64 * 8;
            let ap = unsafe {
                builder
                    .build_gep(
                        i8_type,
                        args_ptr,
                        &[i64_type.const_int(offset, false)],
                        "ap",
                    )
                    .expect("gep")
            };
            let av = builder
                .build_load(i64_type, ap, &format!("arg{i}"))
                .expect("load");

            // If we know the target function's parameter types, coerce appropriately
            let val: BasicValueEnum = if let Some(tf) = target_fn {
                let param_types = tf.get_type().get_param_types();
                if i < param_types.len() {
                    let av_val = av.into_int_value();
                    match param_types[i] {
                        BasicTypeEnum::IntType(it) if it.get_bit_width() < 64 => builder
                            .build_int_truncate(av_val, it, "trunc")
                            .expect("trunc")
                            .into(),
                        BasicTypeEnum::FloatType(ft) => builder
                            .build_bit_cast(av.into_int_value(), ft, "f2i")
                            .expect("bc")
                            .into(),
                        BasicTypeEnum::PointerType(_) => builder
                            .build_int_to_ptr(av_val, ptr_type, "itp")
                            .expect("itp")
                            .into(),
                        _ => av.into(),
                    }
                } else {
                    av.into()
                }
            } else {
                av.into()
            };
            arg_vals.push(val.into());
        }

        // Call the target function via fn_ptr and return the result
        let result = if let Some(tf) = target_fn {
            let call = builder
                .build_direct_call(*tf, &arg_vals, "spawn_result")
                .expect("call");
            call.try_as_basic_value().left()
        } else {
            let fn_type = i64_type.fn_type(&vec![i64_type.into(); site.num_args], false);
            let call = builder
                .build_indirect_call(fn_type, fn_ptr, &arg_vals, "spawn_result")
                .expect("indirect_call");
            call.try_as_basic_value().left()
        };
        if let Some(val) = result {
            // Widen result to i64 for return
            let ret_val: BasicValueEnum = match val {
                BasicValueEnum::IntValue(iv) => {
                    if iv.get_type().get_bit_width() < 64 {
                        builder
                            .build_int_s_extend(iv, i64_type, "widen")
                            .expect("ext")
                            .into()
                    } else {
                        iv.into()
                    }
                }
                BasicValueEnum::FloatValue(fv) => {
                    builder.build_bit_cast(fv, i64_type, "f2i").expect("bc")
                }
                BasicValueEnum::PointerValue(pv) => builder
                    .build_ptr_to_int(pv, i64_type, "p2i")
                    .expect("pti")
                    .into(),
                other => other,
            };
            builder.build_return(Some(&ret_val)).expect("return");
        } else {
            builder
                .build_return(Some(&i64_type.const_int(0, false)))
                .expect("return");
        }
    }

    // Verify the module
    module.verify().map_err(|e| CodegenError {
        code: ErrorCode::E0405,
        message: format!("LLVM module verification failed: {}", e.to_string_lossy()),
    })?;

    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Look up a variant name across all enums and return its tag index.
fn lookup_variant_tag(enum_variants: &HashMap<String, Vec<String>>, name: &str) -> Option<usize> {
    for variants in enum_variants.values() {
        if let Some(idx) = variants.iter().position(|v| v == name) {
            return Some(idx);
        }
    }
    None
}

// ── Expression compilation ──────────────────────────────────────────

fn compile_expr<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    expr: &Spanned<Expr>,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    match &expr.node {
        Expr::IntLit(n) => {
            let val = cx.context.i64_type().const_int(*n as u64, true);
            Ok(Some((val.into(), TurboTy::Int)))
        }

        Expr::FloatLit(f) => {
            let val = cx.context.f64_type().const_float(*f);
            Ok(Some((val.into(), TurboTy::Float)))
        }

        Expr::BoolLit(b) => {
            let val = cx.context.i8_type().const_int(*b as u64, false);
            Ok(Some((val.into(), TurboTy::Bool)))
        }

        Expr::StringLit(s) => {
            let ptr = cx.create_string(s)?;
            Ok(Some((ptr.into(), TurboTy::Str)))
        }

        Expr::Unit => Ok(None),

        Expr::Ident(name) => {
            // Check constants first
            if let Some(const_expr) = cx.constants.get(name.as_str()) {
                let const_expr = const_expr.clone();
                return compile_expr(cx, &const_expr);
            }
            let (alloca, turbo_ty) = cx.vars.get(name).ok_or_else(|| CodegenError {
                code: ErrorCode::E0401,
                message: format!("undefined variable: {name}"),
            })?;
            let turbo_ty = turbo_ty.clone();
            let llvm_ty = turbo_ty_to_llvm_ctx(&turbo_ty, cx.context, cx.enum_max_slots);
            let val = cx
                .builder
                .build_load(llvm_ty, *alloca, name)
                .expect("build_load failed");
            Ok(Some((val, turbo_ty)))
        }

        Expr::BinaryOp { left, op, right } => {
            // Short-circuit for && and ||
            if *op == BinOp::And || *op == BinOp::Or {
                return compile_short_circuit(cx, left, *op, right);
            }

            let (lhs, lhs_tty) = compile_expr(cx, left)?.unwrap();
            let (rhs, rhs_tty) = compile_expr(cx, right)?.unwrap();

            // String operations
            if lhs_tty == TurboTy::Str && rhs_tty == TurboTy::Str {
                match op {
                    BinOp::Add => {
                        let result = cx
                            .rt_call("rt_str_concat", &[lhs.into(), rhs.into()])
                            .unwrap();
                        return Ok(Some((result, TurboTy::Str)));
                    }
                    BinOp::Eq | BinOp::NotEq => {
                        let result = cx.rt_call("rt_str_eq", &[lhs.into(), rhs.into()]).unwrap();
                        let result_int = result.into_int_value();
                        if *op == BinOp::NotEq {
                            let one = cx.context.i8_type().const_int(1, false);
                            let flipped = cx
                                .builder
                                .build_xor(result_int, one, "neq")
                                .expect("build_xor failed");
                            return Ok(Some((flipped.into(), TurboTy::Bool)));
                        }
                        return Ok(Some((result, TurboTy::Bool)));
                    }
                    _ => {}
                }
            }

            // Struct equality: use derived __eq method if available
            if let TurboTy::Struct(ref sname) = lhs_tty {
                if *op == BinOp::Eq || *op == BinOp::NotEq {
                    let eq_fn_name = format!("{sname}__eq");
                    if let Some(&eq_fn) = cx.user_fns.get(&eq_fn_name) {
                        let result = cx
                            .builder
                            .build_direct_call(eq_fn, &[lhs.into(), rhs.into()], "struct_eq")
                            .expect("build_direct_call")
                            .try_as_basic_value()
                            .left()
                            .unwrap();
                        if *op == BinOp::NotEq {
                            let one = cx.context.i8_type().const_int(1, false);
                            let flipped = cx
                                .builder
                                .build_xor(result.into_int_value(), one, "neq")
                                .expect("xor");
                            return Ok(Some((flipped.into(), TurboTy::Bool)));
                        }
                        return Ok(Some((result, TurboTy::Bool)));
                    }
                    // Fallback: pointer comparison
                    let lp = cx
                        .builder
                        .build_ptr_to_int(lhs.into_pointer_value(), cx.context.i64_type(), "lp")
                        .expect("p2i");
                    let rp = cx
                        .builder
                        .build_ptr_to_int(rhs.into_pointer_value(), cx.context.i64_type(), "rp")
                        .expect("p2i");
                    let pred = if *op == BinOp::Eq {
                        IntPredicate::EQ
                    } else {
                        IntPredicate::NE
                    };
                    let cmp = cx
                        .builder
                        .build_int_compare(pred, lp, rp, "ptr_eq")
                        .expect("cmp");
                    return Ok(Some((cmp.into(), TurboTy::Bool)));
                }
            }

            // String coercion: str + non-str or non-str + str
            if *op == BinOp::Add {
                if lhs_tty == TurboTy::Str && rhs_tty != TurboTy::Str {
                    let rhs_str = convert_to_str(cx, rhs, &rhs_tty)?;
                    let result = cx
                        .rt_call("rt_str_concat", &[lhs.into(), rhs_str.into()])
                        .unwrap();
                    return Ok(Some((result, TurboTy::Str)));
                }
                if rhs_tty == TurboTy::Str && lhs_tty != TurboTy::Str {
                    let lhs_str = convert_to_str(cx, lhs, &lhs_tty)?;
                    let result = cx
                        .rt_call("rt_str_concat", &[lhs_str.into(), rhs.into()])
                        .unwrap();
                    return Ok(Some((result, TurboTy::Str)));
                }
            }

            let result = compile_binop(cx, lhs, *op, rhs)?;
            let result_tty = match op {
                BinOp::Eq
                | BinOp::NotEq
                | BinOp::Less
                | BinOp::LessEq
                | BinOp::Greater
                | BinOp::GreaterEq
                | BinOp::And
                | BinOp::Or => TurboTy::Bool,
                _ => lhs_tty,
            };
            Ok(Some((result, result_tty)))
        }

        Expr::UnaryOp { op, expr: inner } => {
            let (val, tty) = compile_expr(cx, inner)?.unwrap();
            let result = match op {
                UnaryOp::Neg => match val {
                    BasicValueEnum::FloatValue(fv) => cx
                        .builder
                        .build_float_neg(fv, "fneg")
                        .expect("build_float_neg failed")
                        .into(),
                    BasicValueEnum::IntValue(iv) => cx
                        .builder
                        .build_int_neg(iv, "ineg")
                        .expect("build_int_neg failed")
                        .into(),
                    _ => {
                        return Err(CodegenError {
                            code: ErrorCode::E0403,
                            message: "cannot negate this type".to_string(),
                        })
                    }
                },
                UnaryOp::Not => {
                    let iv = val.into_int_value();
                    let one = cx.context.i8_type().const_int(1, false);
                    cx.builder
                        .build_xor(iv, one, "not")
                        .expect("build_xor failed")
                        .into()
                }
            };
            let result_tty = match op {
                UnaryOp::Not => TurboTy::Bool,
                UnaryOp::Neg => tty,
            };
            Ok(Some((result, result_tty)))
        }

        Expr::Call { callee, args } => compile_call(cx, callee, args),

        Expr::If {
            condition,
            then_branch,
            else_branch,
        } => compile_if(cx, condition, then_branch, else_branch.as_deref()),

        Expr::IfLet { .. } => {
            // TODO: if-let not yet implemented for LLVM backend
            Err(CodegenError {
                code: ErrorCode::E0400,
                message: "if-let is not yet supported in the LLVM backend".to_string(),
            })
        }

        Expr::Block { stmts, tail_expr } => {
            let saved_vars = cx.vars.clone();

            let mut deferred: Vec<&Spanned<Expr>> = Vec::new();
            for stmt in stmts {
                if let Stmt::Defer(ref defer_expr) = stmt.node {
                    deferred.push(defer_expr);
                }
                compile_stmt(cx, stmt)?;
            }
            let result = if let Some(tail) = tail_expr {
                compile_expr(cx, tail)
            } else {
                Ok(None)
            };

            // Emit deferred expressions in LIFO order
            for defer_expr in deferred.iter().rev() {
                let block = cx.builder.get_insert_block().unwrap();
                if block.get_terminator().is_none() {
                    compile_expr(cx, defer_expr)?;
                }
            }

            cx.vars = saved_vars;
            result
        }

        Expr::Assign { target, value } => {
            let (val, tty) = compile_expr(cx, value)?.unwrap();
            let (alloca, _) = cx.vars.get(target).ok_or_else(|| CodegenError {
                code: ErrorCode::E0401,
                message: format!("undefined variable: {target}"),
            })?;
            let alloca = *alloca;
            cx.builder
                .build_store(alloca, val)
                .expect("build_store failed");
            // Update type
            if let Some(entry) = cx.vars.get_mut(target) {
                entry.1 = tty;
            }
            Ok(None)
        }

        Expr::CompoundAssign { target, op, value } => {
            let (rhs, _) = compile_expr(cx, value)?.unwrap();
            let (alloca, turbo_ty) = cx.vars.get(target).ok_or_else(|| CodegenError {
                code: ErrorCode::E0401,
                message: format!("undefined variable: {target}"),
            })?;
            let alloca = *alloca;
            let turbo_ty = turbo_ty.clone();
            let llvm_ty = turbo_ty_to_llvm_ctx(&turbo_ty, cx.context, cx.enum_max_slots);
            let lhs = cx
                .builder
                .build_load(llvm_ty, alloca, target)
                .expect("build_load failed");
            let result = compile_binop(cx, lhs, *op, rhs)?;
            cx.builder
                .build_store(alloca, result)
                .expect("build_store failed");
            Ok(None)
        }

        Expr::FieldAssign {
            object,
            field,
            value,
        } => {
            let (obj_ptr, obj_tty) = compile_expr(cx, object)?.unwrap();
            let (val, _) = compile_expr(cx, value)?.unwrap();

            let struct_name = match &obj_tty {
                TurboTy::Struct(name) => name.clone(),
                _ => {
                    return Err(CodegenError {
                        code: ErrorCode::E0400,
                        message: "field assignment on non-struct type".to_string(),
                    })
                }
            };

            let struct_layout = cx
                .struct_fields
                .get(&struct_name)
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: format!("undefined struct: {struct_name}"),
                })?
                .clone();

            let field_index = struct_layout
                .iter()
                .position(|(n, _)| n == field)
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: format!("struct `{struct_name}` has no field `{field}`"),
                })?;

            let offset = field_index as u64 * 8;
            let obj_ptr_val = obj_ptr.into_pointer_value();

            // GEP to field offset
            let field_ptr = unsafe {
                cx.builder
                    .build_gep(
                        cx.context.i8_type(),
                        obj_ptr_val,
                        &[cx.context.i64_type().const_int(offset, false)],
                        "field_ptr",
                    )
                    .expect("build_gep failed")
            };

            // Widen to i64 for uniform storage
            let store_val = widen_for_storage(cx, val);
            cx.builder
                .build_store(field_ptr, store_val)
                .expect("build_store failed");
            Ok(None)
        }

        Expr::IndexAssign {
            object,
            index,
            value,
        } => {
            let (arr, _) = compile_expr(cx, object)?.unwrap();
            let (idx, _) = compile_expr(cx, index)?.unwrap();
            let (val, _) = compile_expr(cx, value)?.unwrap();

            let store_val = widen_for_storage(cx, val);
            let new_arr = cx
                .rt_call("rt_array_set", &[arr.into(), idx.into(), store_val.into()])
                .unwrap();

            // Update the variable to point to the (possibly new) array
            if let Expr::Ident(name) = &object.node {
                if let Some((alloca, _)) = cx.vars.get(name) {
                    cx.builder
                        .build_store(*alloca, new_arr)
                        .expect("build_store failed");
                }
            }

            Ok(None)
        }

        Expr::While { condition, body } => compile_while(cx, condition, body),

        Expr::ForIn {
            var_name,
            iterable,
            body,
        } => compile_for_in(cx, var_name, iterable, body),

        Expr::ArrayLit(elems) => {
            let len = elems.len() as u64;
            let len_val = cx.context.i64_type().const_int(len, false);
            let arr = cx.rt_call("rt_array_alloc", &[len_val.into()]).unwrap();

            let mut elem_tty = TurboTy::Int;
            for (i, elem) in elems.iter().enumerate() {
                let (val, tty) = compile_expr(cx, elem)?.unwrap();
                if i == 0 {
                    elem_tty = tty;
                }
                let idx = cx.context.i64_type().const_int(i as u64, false);
                let store_val = widen_for_storage(cx, val);
                cx.rt_call("rt_array_set", &[arr.into(), idx.into(), store_val.into()]);
            }

            Ok(Some((arr, TurboTy::Array(Box::new(elem_tty)))))
        }

        Expr::Index { object, index } => {
            let (obj, obj_tty) = compile_expr(cx, object)?.unwrap();
            let (idx, _) = compile_expr(cx, index)?.unwrap();

            let elem_tty = match &obj_tty {
                TurboTy::Array(inner) => *inner.clone(),
                _ => TurboTy::Int,
            };

            let raw = cx
                .rt_call("rt_array_get", &[obj.into(), idx.into()])
                .unwrap();

            // Narrow the result back from i64 to the element type
            let result = narrow_from_storage(cx, raw, &elem_tty);
            Ok(Some((result, elem_tty)))
        }

        Expr::StructLit { name, fields } => {
            // Check if this is an agent instantiation
            if let Some((model, tools, system_prompt)) = cx.agent_defs.get(name).cloned() {
                let i64_type = cx.context.i64_type();
                let ptr_type = cx.context.ptr_type(AddressSpace::default());
                let i8_type = cx.context.i8_type();
                let num_fields_val = i64_type.const_int(3, false);
                let ptr = cx
                    .rt_call("rt_struct_alloc", &[num_fields_val.into()])
                    .unwrap()
                    .into_pointer_value();

                // Slot 0: model string
                let model_ptr = cx.create_string(&model)?;
                let model_i64 = cx
                    .builder
                    .build_ptr_to_int(model_ptr, i64_type, "model_i64")
                    .expect("pti");
                cx.builder.build_store(ptr, model_i64).expect("store");

                // Slot 1: system prompt string
                let system_str = system_prompt.as_deref().unwrap_or("");
                let system_ptr = cx.create_string(system_str)?;
                let system_i64 = cx
                    .builder
                    .build_ptr_to_int(system_ptr, i64_type, "sys_i64")
                    .expect("pti");
                let sys_field = unsafe {
                    cx.builder
                        .build_gep(i8_type, ptr, &[i64_type.const_int(8, false)], "sys_ptr")
                        .expect("gep")
                };
                cx.builder
                    .build_store(sys_field, system_i64)
                    .expect("store");

                // Slot 2: tools array
                let tools_len = i64_type.const_int(tools.len() as u64, false);
                let arr_ptr = cx
                    .rt_call("rt_array_alloc", &[tools_len.into()])
                    .unwrap()
                    .into_pointer_value();
                for (i, tool_name) in tools.iter().enumerate() {
                    let tool_str = cx.create_string(tool_name)?;
                    let idx = i64_type.const_int(i as u64, false);
                    let tool_i64 = cx
                        .builder
                        .build_ptr_to_int(tool_str, i64_type, "tool_i64")
                        .expect("pti");
                    cx.rt_call(
                        "rt_array_set",
                        &[arr_ptr.into(), idx.into(), tool_i64.into()],
                    );
                }
                let arr_i64 = cx
                    .builder
                    .build_ptr_to_int(arr_ptr, i64_type, "arr_i64")
                    .expect("pti");
                let tools_field = unsafe {
                    cx.builder
                        .build_gep(i8_type, ptr, &[i64_type.const_int(16, false)], "tools_ptr")
                        .expect("gep")
                };
                cx.builder.build_store(tools_field, arr_i64).expect("store");

                return Ok(Some((ptr.into(), TurboTy::Agent(name.clone()))));
            }

            let struct_layout = cx
                .struct_fields
                .get(name)
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: format!("undefined struct: {name}"),
                })?
                .clone();

            let num_fields = struct_layout.len() as u64;
            let num_fields_val = cx.context.i64_type().const_int(num_fields, false);
            let ptr = cx
                .rt_call("rt_struct_alloc", &[num_fields_val.into()])
                .unwrap()
                .into_pointer_value();

            let mut concrete_fields: Vec<(String, TurboTy)> = Vec::new();
            for (field_name, field_expr) in fields {
                let (val, val_tty) = compile_expr(cx, field_expr)?.unwrap();
                concrete_fields.push((field_name.clone(), val_tty));
                let field_index = struct_layout
                    .iter()
                    .position(|(n, _)| n == field_name)
                    .ok_or_else(|| CodegenError {
                        code: ErrorCode::E0400,
                        message: format!("struct `{name}` has no field `{field_name}`"),
                    })?;

                let offset = field_index as u64 * 8;
                let field_ptr = unsafe {
                    cx.builder
                        .build_gep(
                            cx.context.i8_type(),
                            ptr,
                            &[cx.context.i64_type().const_int(offset, false)],
                            "field_ptr",
                        )
                        .expect("build_gep failed")
                };

                let store_val = widen_for_storage(cx, val);
                cx.builder
                    .build_store(field_ptr, store_val)
                    .expect("build_store failed");
            }

            let result_tty = if cx.agent_names.contains(name) {
                TurboTy::Agent(name.clone())
            } else {
                TurboTy::Struct(name.clone())
            };
            // Store concrete field types for generic struct tracking
            // Use a temp key "__last_struct_lit" that Let binding will pick up
            if !concrete_fields.is_empty() {
                cx.concrete_struct_fields
                    .insert("__last_struct_lit".to_string(), concrete_fields);
            }
            Ok(Some((ptr.into(), result_tty)))
        }

        Expr::FieldAccess { object, field } => {
            // Check if this is an enum variant access: EnumName.VariantName
            if let Expr::Ident(ref name) = object.node {
                if let Some(variants) = cx.enum_variants.get(name.as_str()) {
                    let index =
                        variants
                            .iter()
                            .position(|v| v == field)
                            .ok_or_else(|| CodegenError {
                                code: ErrorCode::E0400,
                                message: format!("enum `{name}` has no variant `{field}`"),
                            })?;

                    if let Some(&max_slots) = cx.enum_max_slots.get(name.as_str()) {
                        // Data-carrying enum: allocate tagged union
                        let total_slots = 1 + max_slots;
                        let num_fields_val =
                            cx.context.i64_type().const_int(total_slots as u64, false);
                        let ptr = cx
                            .rt_call("rt_struct_alloc", &[num_fields_val.into()])
                            .unwrap()
                            .into_pointer_value();
                        let tag_val = cx.context.i64_type().const_int(index as u64, false);
                        cx.builder
                            .build_store(ptr, tag_val)
                            .expect("build_store failed");
                        return Ok(Some((ptr.into(), TurboTy::Enum(name.clone()))));
                    } else {
                        let val = cx.context.i64_type().const_int(index as u64, false);
                        return Ok(Some((val.into(), TurboTy::Enum(name.clone()))));
                    }
                }
            }

            let (obj, obj_tty) = compile_expr(cx, object)?.unwrap();

            // Handle agent field access: model (slot 0), system (slot 1), tools (slot 2)
            if let TurboTy::Agent(_) = &obj_tty {
                let (offset, tty) = match field.as_str() {
                    "model" => (0u64, TurboTy::Str),
                    "system" => (8u64, TurboTy::Str),
                    "tools" => (16u64, TurboTy::Array(Box::new(TurboTy::Str))),
                    _ => {
                        return Err(CodegenError {
                            code: ErrorCode::E0400,
                            message: format!("agent has no field `{field}`"),
                        })
                    }
                };
                let obj_ptr = obj.into_pointer_value();
                let field_ptr = if offset == 0 {
                    obj_ptr
                } else {
                    unsafe {
                        cx.builder
                            .build_gep(
                                cx.context.i8_type(),
                                obj_ptr,
                                &[cx.context.i64_type().const_int(offset, false)],
                                "agent_field_ptr",
                            )
                            .expect("gep")
                    }
                };
                let val = cx
                    .builder
                    .build_load(cx.context.i64_type(), field_ptr, "agent_field")
                    .expect("load");
                // For Str/Array fields, the loaded i64 is actually a pointer
                let val = if matches!(tty, TurboTy::Str | TurboTy::Array(_)) {
                    cx.builder
                        .build_int_to_ptr(
                            val.into_int_value(),
                            cx.context.ptr_type(AddressSpace::default()),
                            "field_ptr",
                        )
                        .expect("itp")
                        .into()
                } else {
                    val
                };
                return Ok(Some((val, tty)));
            }

            let struct_name = match &obj_tty {
                TurboTy::Struct(name) => name.clone(),
                _ => {
                    return Err(CodegenError {
                        code: ErrorCode::E0400,
                        message: format!("field access on non-struct type: {field}"),
                    })
                }
            };

            let struct_layout = cx
                .struct_fields
                .get(&struct_name)
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: format!("undefined struct: {struct_name}"),
                })?
                .clone();

            let (field_index, (_, field_tty)) = struct_layout
                .iter()
                .enumerate()
                .find(|(_, (n, _))| n == field)
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: format!("struct `{struct_name}` has no field `{field}`"),
                })?;

            // Check if we have concrete field types (from generic struct instantiation)
            let concrete_tty = if let Expr::Ident(ref var_name) = object.node {
                cx.concrete_struct_fields.get(var_name).and_then(|fields| {
                    fields
                        .iter()
                        .find(|(n, _)| n == field)
                        .map(|(_, t)| t.clone())
                })
            } else {
                None
            };
            let field_tty = concrete_tty.unwrap_or_else(|| field_tty.clone());

            let offset = field_index as u64 * 8;
            let obj_ptr = obj.into_pointer_value();

            let field_ptr = unsafe {
                cx.builder
                    .build_gep(
                        cx.context.i8_type(),
                        obj_ptr,
                        &[cx.context.i64_type().const_int(offset, false)],
                        "field_ptr",
                    )
                    .expect("build_gep failed")
            };

            // Load as i64 then narrow to the field type
            let raw = cx
                .builder
                .build_load(cx.context.i64_type(), field_ptr, field)
                .expect("build_load failed");
            let result = narrow_from_storage(cx, raw, &field_tty);
            Ok(Some((result, field_tty)))
        }

        Expr::EnumVariant { enum_name, variant } => {
            let variants = cx
                .enum_variants
                .get(enum_name)
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: format!("undefined enum: {enum_name}"),
                })?;
            let variant_index =
                variants
                    .iter()
                    .position(|v| v == variant)
                    .ok_or_else(|| CodegenError {
                        code: ErrorCode::E0400,
                        message: format!("enum `{enum_name}` has no variant `{variant}`"),
                    })?;
            let val = cx.context.i64_type().const_int(variant_index as u64, false);
            Ok(Some((val.into(), TurboTy::Enum(enum_name.clone()))))
        }

        Expr::Match { subject, arms } => compile_match(cx, subject, arms),

        Expr::Interpolation(parts) => {
            let empty_str = cx.create_string("")?;
            let mut result: BasicValueEnum<'ctx> = empty_str.into();

            for part in parts {
                match part {
                    InterpolPart::Lit(s) => {
                        let lit_ptr = cx.create_string(s)?;
                        result = cx
                            .rt_call("rt_str_concat", &[result.into(), lit_ptr.into()])
                            .unwrap();
                    }
                    InterpolPart::Expr(e) => {
                        let (val, tty) = compile_expr(cx, e)?.unwrap();
                        let str_val = convert_to_str(cx, val, &tty)?;
                        result = cx
                            .rt_call("rt_str_concat", &[result.into(), str_val.into()])
                            .unwrap();
                    }
                }
            }

            Ok(Some((result, TurboTy::Str)))
        }

        Expr::Range { start, end } => {
            // Ranges are only used inside for-in, but if used standalone, return a tuple-like thing
            let (start_val, _) = compile_expr(cx, start)?.unwrap();
            let (end_val, _) = compile_expr(cx, end)?.unwrap();
            // Store as array [start, end]
            let len_val = cx.context.i64_type().const_int(2, false);
            let arr = cx.rt_call("rt_array_alloc", &[len_val.into()]).unwrap();
            let idx0 = cx.context.i64_type().const_int(0, false);
            let idx1 = cx.context.i64_type().const_int(1, false);
            cx.rt_call("rt_array_set", &[arr.into(), idx0.into(), start_val.into()]);
            cx.rt_call("rt_array_set", &[arr.into(), idx1.into(), end_val.into()]);
            Ok(Some((arr, TurboTy::Array(Box::new(TurboTy::Int)))))
        }

        Expr::OkExpr(inner) => {
            let (val, _) = compile_expr(cx, inner)?.unwrap();
            let val_i64 = widen_for_storage(cx, val);
            let result = cx.rt_call("rt_result_ok", &[val_i64.into()]).unwrap();
            Ok(Some((
                result,
                TurboTy::Result(Box::new(TurboTy::Int), Box::new(TurboTy::Str)),
            )))
        }

        Expr::ErrExpr(inner) => {
            let (val, _) = compile_expr(cx, inner)?.unwrap();
            let val_i64 = widen_for_storage(cx, val);
            let result = cx.rt_call("rt_result_err", &[val_i64.into()]).unwrap();
            Ok(Some((
                result,
                TurboTy::Result(Box::new(TurboTy::Int), Box::new(TurboTy::Str)),
            )))
        }

        Expr::SomeExpr(inner) => {
            let (val, _) = compile_expr(cx, inner)?.unwrap();
            let val_i64 = widen_for_storage(cx, val);
            let result = cx.rt_call("rt_option_some", &[val_i64.into()]).unwrap();
            Ok(Some((result, TurboTy::Optional(Box::new(TurboTy::Int)))))
        }

        Expr::NoneExpr => {
            let result = cx.rt_call("rt_option_none", &[]).unwrap();
            Ok(Some((result, TurboTy::Optional(Box::new(TurboTy::Int)))))
        }

        Expr::NullCoalesce { value, default } => {
            let (val, val_tty) = compile_expr(cx, value)?.unwrap();
            // Get the tag
            let tag = cx
                .rt_call("rt_option_tag", &[val.into()])
                .unwrap()
                .into_int_value();
            let zero = cx.context.i64_type().const_int(0, false);
            let is_none = cx
                .builder
                .build_int_compare(IntPredicate::EQ, tag, zero, "is_none")
                .expect("build_int_compare failed");

            let then_block = cx
                .context
                .append_basic_block(cx.current_fn, "coalesce_none");
            let else_block = cx
                .context
                .append_basic_block(cx.current_fn, "coalesce_some");
            let merge_block = cx
                .context
                .append_basic_block(cx.current_fn, "coalesce_merge");

            cx.builder
                .build_conditional_branch(is_none, then_block, else_block)
                .expect("build_conditional_branch failed");

            // None case: use default
            cx.builder.position_at_end(then_block);
            let (default_val, default_tty) = compile_expr(cx, default)?.unwrap();
            let then_end_block = cx.builder.get_insert_block().unwrap();
            cx.builder
                .build_unconditional_branch(merge_block)
                .expect("build_unconditional_branch failed");

            // Some case: unwrap
            cx.builder.position_at_end(else_block);
            let unwrapped = cx.rt_call("rt_option_value", &[val.into()]).unwrap();
            let inner_tty = match &val_tty {
                TurboTy::Optional(inner) => *inner.clone(),
                _ => TurboTy::Int,
            };
            let unwrapped = narrow_from_storage(cx, unwrapped, &inner_tty);
            let else_end_block = cx.builder.get_insert_block().unwrap();
            cx.builder
                .build_unconditional_branch(merge_block)
                .expect("build_unconditional_branch failed");

            cx.builder.position_at_end(merge_block);
            let phi = cx
                .builder
                .build_phi(default_val.get_type(), "coalesce")
                .expect("build_phi failed");
            phi.add_incoming(&[(&default_val, then_end_block), (&unwrapped, else_end_block)]);

            Ok(Some((phi.as_basic_value(), default_tty)))
        }

        Expr::OptionalChain { .. } => {
            // TODO: implement optional chaining in LLVM backend
            Err(CodegenError {
                code: ErrorCode::E0400,
                message: "optional chaining `?.` is not yet supported in the LLVM backend"
                    .to_string(),
            })
        }

        Expr::Closure { params, .. } => {
            // Look up the pre-extracted closure function by span start
            let span_start = expr.span.start;
            let (closure_name, closure_ty, free_vars) = cx
                .closure_fns
                .get(&span_start)
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: "internal error: closure not found in pre-compiled map".to_string(),
                })?
                .clone();

            let func = *cx
                .user_fns
                .get(closure_name.as_str())
                .ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: format!(
                        "internal error: closure function {} not found",
                        closure_name
                    ),
                })?;

            // Get the function pointer as an i64 (pointer-sized integer)
            let fn_ptr = func.as_global_value().as_pointer_value();

            // Determine captures: free variables that actually exist in scope
            let mut bound_params: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
            let mut all_free: Vec<String> = Vec::new();
            collect_free_vars_llvm(&expr.node, &mut bound_params, &mut all_free);
            let capture_names: Vec<String> = free_vars
                .iter()
                .filter(|n| cx.vars.contains_key(*n))
                .cloned()
                .collect();

            let ptr_type = cx.context.ptr_type(AddressSpace::default());
            let i8_type = cx.context.i8_type();
            let i64_type = cx.context.i64_type();

            // Allocate environment struct for captured variables
            let env_ptr = if !capture_names.is_empty() {
                let num_captures = i64_type.const_int(capture_names.len() as u64, false);
                let env_ptr = cx
                    .rt_call("rt_struct_alloc", &[num_captures.into()])
                    .unwrap()
                    .into_pointer_value();

                // Store each captured variable into the env struct
                for (cap_idx, cap_name) in capture_names.iter().enumerate() {
                    let (alloca, cap_tty) = cx
                        .vars
                        .get(cap_name)
                        .ok_or_else(|| CodegenError {
                            code: ErrorCode::E0400,
                            message: format!("capture variable {} not found", cap_name),
                        })?
                        .clone();
                    let val = cx
                        .builder
                        .build_load(
                            turbo_ty_to_llvm_ctx(&cap_tty, cx.context, cx.enum_max_slots),
                            alloca,
                            cap_name,
                        )
                        .expect("build_load failed");
                    let val_i64 = widen_for_storage(cx, val);
                    let offset = (cap_idx as u64) * 8;
                    let field_ptr = unsafe {
                        cx.builder
                            .build_gep(
                                i8_type,
                                env_ptr,
                                &[i64_type.const_int(offset, false)],
                                "cap_ptr",
                            )
                            .expect("build_gep failed")
                    };
                    cx.builder
                        .build_store(field_ptr, val_i64)
                        .expect("build_store failed");
                }
                env_ptr.into()
            } else {
                // No captures: null pointer
                ptr_type.const_null().into()
            };

            // Allocate closure pair: [fn_ptr_as_i64, env_ptr_as_i64]
            let two = i64_type.const_int(2, false);
            let closure_ptr = cx
                .rt_call("rt_struct_alloc", &[two.into()])
                .unwrap()
                .into_pointer_value();

            // Store fn_ptr at slot 0 (as i64)
            let fn_ptr_i64 = cx
                .builder
                .build_ptr_to_int(fn_ptr, i64_type, "fn_ptr_i64")
                .expect("build_ptr_to_int failed");
            cx.builder
                .build_store(closure_ptr, fn_ptr_i64)
                .expect("build_store failed");

            // Store env_ptr at slot 1 (offset 8)
            let env_slot = unsafe {
                cx.builder
                    .build_gep(
                        i8_type,
                        closure_ptr,
                        &[i64_type.const_int(8, false)],
                        "env_slot",
                    )
                    .expect("build_gep failed")
            };
            let env_i64: BasicValueEnum = match env_ptr {
                BasicValueEnum::PointerValue(pv) => cx
                    .builder
                    .build_ptr_to_int(pv, i64_type, "env_i64")
                    .expect("pti")
                    .into(),
                other => other,
            };
            cx.builder
                .build_store(env_slot, env_i64)
                .expect("build_store failed");

            Ok(Some((closure_ptr.into(), closure_ty)))
        }

        Expr::Await(inner) => {
            let result = compile_expr(cx, inner)?;
            if let Some((val, tty)) = result {
                match tty {
                    TurboTy::Future(inner_tty) => {
                        let joined = cx.rt_call("rt_await_handle", &[val.into()]).unwrap();
                        let narrowed = narrow_from_storage(cx, joined, &inner_tty);
                        Ok(Some((narrowed, *inner_tty)))
                    }
                    _ => Ok(Some((val, tty))),
                }
            } else {
                Ok(None)
            }
        }

        Expr::Spawn(inner) => {
            let span_start = expr.span.start;
            if let Some(thunk_name) = cx.spawn_thunks.get(&span_start).cloned() {
                if let Expr::Call { callee, args } = &inner.node {
                    if let Expr::Ident(callee_name) = &callee.node {
                        let inner_ret_tty = cx
                            .fn_ret_types
                            .get(callee_name.as_str())
                            .cloned()
                            .unwrap_or(TurboTy::Unit);

                        let target_func =
                            *cx.user_fns
                                .get(callee_name.as_str())
                                .ok_or_else(|| CodegenError {
                                    code: ErrorCode::E0402,
                                    message: format!("spawn: unknown function `{}`", callee_name),
                                })?;
                        let target_fn_ptr = target_func.as_global_value().as_pointer_value();

                        // Compile all arguments
                        let mut arg_vals: Vec<BasicValueEnum> = Vec::new();
                        for arg in args {
                            if let Some((val, _tty)) = compile_expr(cx, arg)? {
                                let val_i64 = widen_for_storage(cx, val);
                                arg_vals.push(val_i64.into());
                            }
                        }

                        let i8_type = cx.context.i8_type();
                        let i64_type = cx.context.i64_type();
                        let ptr_type = cx.context.ptr_type(AddressSpace::default());

                        // Allocate args struct: [fn_ptr, arg0, arg1, ...]
                        let num_slots = i64_type.const_int((1 + arg_vals.len()) as u64, false);
                        let args_ptr = cx
                            .rt_call("rt_struct_alloc", &[num_slots.into()])
                            .unwrap()
                            .into_pointer_value();

                        // Store fn_ptr at offset 0
                        let fn_ptr_i64 = cx
                            .builder
                            .build_ptr_to_int(target_fn_ptr, i64_type, "spawn_fn_i64")
                            .expect("pti");
                        cx.builder.build_store(args_ptr, fn_ptr_i64).expect("store");

                        // Store args at offsets 8, 16, 24, ...
                        for (i, val) in arg_vals.iter().enumerate() {
                            let offset = ((i + 1) * 8) as u64;
                            let slot = unsafe {
                                cx.builder
                                    .build_gep(
                                        i8_type,
                                        args_ptr,
                                        &[i64_type.const_int(offset, false)],
                                        "arg_slot",
                                    )
                                    .expect("gep")
                            };
                            cx.builder.build_store(slot, *val).expect("store");
                        }

                        // Get the thunk function address
                        let thunk_func =
                            *cx.user_fns
                                .get(thunk_name.as_str())
                                .ok_or_else(|| CodegenError {
                                    code: ErrorCode::E0405,
                                    message: format!("spawn: thunk `{}` not found", thunk_name),
                                })?;
                        let thunk_fn_ptr = thunk_func.as_global_value().as_pointer_value();
                        let thunk_fn_i64 = cx
                            .builder
                            .build_ptr_to_int(thunk_fn_ptr, i64_type, "thunk_fn_i64")
                            .expect("pti");

                        // rt_spawn_with_args(thunk_ptr: ptr, args_ptr: ptr) -> ptr (handle)
                        // Store thunk ptr as pointer value directly
                        let handle = cx
                            .rt_call(
                                "rt_spawn_with_args",
                                &[thunk_fn_ptr.into(), args_ptr.into()],
                            )
                            .unwrap();

                        return Ok(Some((handle, TurboTy::Future(Box::new(inner_ret_tty)))));
                    }
                }
            }
            // Fallback: compile inner expression synchronously
            compile_expr(cx, inner)
        }

        Expr::Try(inner) => {
            let (val, val_tty) = compile_expr(cx, inner)?.unwrap();
            // Check tag: 0 = Ok, 1 = Err
            let tag = cx
                .rt_call("rt_result_tag", &[val.into()])
                .unwrap()
                .into_int_value();
            let one = cx.context.i64_type().const_int(1, false);
            let is_err = cx
                .builder
                .build_int_compare(IntPredicate::EQ, tag, one, "is_err")
                .expect("build_int_compare failed");

            let err_block = cx.context.append_basic_block(cx.current_fn, "try_err");
            let ok_block = cx.context.append_basic_block(cx.current_fn, "try_ok");

            cx.builder
                .build_conditional_branch(is_err, err_block, ok_block)
                .expect("build_conditional_branch failed");

            // Error path: propagate
            cx.builder.position_at_end(err_block);
            let err_val = cx.rt_call("rt_result_value", &[val.into()]).unwrap();
            let err_result = cx.rt_call("rt_result_err", &[err_val.into()]).unwrap();
            cx.builder
                .build_return(Some(&err_result))
                .expect("build_return failed");

            // Ok path: unwrap
            cx.builder.position_at_end(ok_block);
            let ok_val = cx.rt_call("rt_result_value", &[val.into()]).unwrap();
            let inner_tty = match &val_tty {
                TurboTy::Result(ok, _) => *ok.clone(),
                _ => TurboTy::Int,
            };
            let narrowed = narrow_from_storage(cx, ok_val, &inner_tty);
            Ok(Some((narrowed, inner_tty)))
        }

        Expr::Break => {
            if let Some((_, exit_block)) = cx.loop_stack.last() {
                cx.builder
                    .build_unconditional_branch(*exit_block)
                    .expect("build_unconditional_branch failed");
                // Create unreachable block for subsequent code
                let dead_block = cx.context.append_basic_block(cx.current_fn, "after_break");
                cx.builder.position_at_end(dead_block);
            }
            Ok(None)
        }

        Expr::Continue => {
            if let Some((header_block, _)) = cx.loop_stack.last() {
                cx.builder
                    .build_unconditional_branch(*header_block)
                    .expect("build_unconditional_branch failed");
                let dead_block = cx
                    .context
                    .append_basic_block(cx.current_fn, "after_continue");
                cx.builder.position_at_end(dead_block);
            }
            Ok(None)
        }
    }
}

// ── Statement compilation ───────────────────────────────────────────

fn compile_stmt<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    stmt: &Spanned<Stmt>,
) -> Result<(), CodegenError> {
    match &stmt.node {
        Stmt::Let { name, value, .. } => {
            let rhs_is_ident = matches!(&value.node, Expr::Ident(_));
            let result = compile_expr(cx, value)?;
            let (llvm_ty, turbo_ty, val) = if let Some((v, tty)) = result {
                (v.get_type(), tty, Some(v))
            } else {
                (
                    cx.context.i64_type().as_basic_type_enum(),
                    TurboTy::Unit,
                    None,
                )
            };
            // COW: if RHS is another variable with a heap type, increment refcount
            if rhs_is_ident {
                if let Some(v) = val {
                    let needs_retain = matches!(
                        &turbo_ty,
                        TurboTy::Array(_)
                            | TurboTy::Struct(_)
                            | TurboTy::Result(_, _)
                            | TurboTy::Optional(_)
                    );
                    if needs_retain && v.is_pointer_value() {
                        cx.rt_call("rt_retain", &[v.into()]);
                    }
                }
            }
            let alloca = cx.create_entry_block_alloca(llvm_ty, name);
            if let Some(v) = val {
                cx.builder
                    .build_store(alloca, v)
                    .expect("build_store failed");
            }
            cx.vars.insert(name.clone(), (alloca, turbo_ty));
            // Transfer concrete struct field types from StructLit
            if let Some(fields) = cx.concrete_struct_fields.remove("__last_struct_lit") {
                cx.concrete_struct_fields.insert(name.clone(), fields);
            }
            Ok(())
        }
        Stmt::Expr(e) => {
            compile_expr(cx, e)?;
            Ok(())
        }
        Stmt::Return(value) => {
            if let Some(val_expr) = value {
                let result = compile_expr(cx, val_expr)?;
                if let Some((v, _)) = result {
                    cx.builder
                        .build_return(Some(&v))
                        .expect("build_return failed");
                } else {
                    cx.builder.build_return(None).expect("build_return failed");
                }
            } else {
                cx.builder.build_return(None).expect("build_return failed");
            }
            // Create dead block for subsequent code
            let dead_block = cx.context.append_basic_block(cx.current_fn, "after_return");
            cx.builder.position_at_end(dead_block);
            Ok(())
        }
        Stmt::Defer(_) => {
            // Handled at block level
            Ok(())
        }
        Stmt::LetDestructure { .. } => {
            // TODO: implement struct destructuring for LLVM backend
            Ok(())
        }
    }
}

// ── Binary operations ───────────────────────────────────────────────

fn compile_binop<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    lhs: BasicValueEnum<'ctx>,
    op: BinOp,
    rhs: BasicValueEnum<'ctx>,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    // Float operations
    if let (BasicValueEnum::FloatValue(lf), BasicValueEnum::FloatValue(rf)) = (lhs, rhs) {
        let result = match op {
            BinOp::Add => cx
                .builder
                .build_float_add(lf, rf, "fadd")
                .expect("build_float_add failed")
                .into(),
            BinOp::Sub => cx
                .builder
                .build_float_sub(lf, rf, "fsub")
                .expect("build_float_sub failed")
                .into(),
            BinOp::Mul => cx
                .builder
                .build_float_mul(lf, rf, "fmul")
                .expect("build_float_mul failed")
                .into(),
            BinOp::Div => cx
                .builder
                .build_float_div(lf, rf, "fdiv")
                .expect("build_float_div failed")
                .into(),
            BinOp::Mod => cx
                .builder
                .build_float_rem(lf, rf, "fmod")
                .expect("build_float_rem failed")
                .into(),
            BinOp::Eq => cx
                .builder
                .build_float_compare(FloatPredicate::OEQ, lf, rf, "feq")
                .expect("build_float_compare failed")
                .into(),
            BinOp::NotEq => cx
                .builder
                .build_float_compare(FloatPredicate::ONE, lf, rf, "fneq")
                .expect("build_float_compare failed")
                .into(),
            BinOp::Less => cx
                .builder
                .build_float_compare(FloatPredicate::OLT, lf, rf, "flt")
                .expect("build_float_compare failed")
                .into(),
            BinOp::LessEq => cx
                .builder
                .build_float_compare(FloatPredicate::OLE, lf, rf, "fle")
                .expect("build_float_compare failed")
                .into(),
            BinOp::Greater => cx
                .builder
                .build_float_compare(FloatPredicate::OGT, lf, rf, "fgt")
                .expect("build_float_compare failed")
                .into(),
            BinOp::GreaterEq => cx
                .builder
                .build_float_compare(FloatPredicate::OGE, lf, rf, "fge")
                .expect("build_float_compare failed")
                .into(),
            _ => {
                return Err(CodegenError {
                    code: ErrorCode::E0403,
                    message: format!("unsupported float op: {op:?}"),
                })
            }
        };
        // Widen i1 comparison results to i8 for consistent Bool representation
        let result = widen_i1_to_i8(cx, result);
        return Ok(result);
    }

    // Integer operations
    let li = lhs.into_int_value();
    let ri = rhs.into_int_value();

    // Widen mismatched widths
    let (li, ri) = if li.get_type().get_bit_width() != ri.get_type().get_bit_width() {
        let target_bits = li
            .get_type()
            .get_bit_width()
            .max(ri.get_type().get_bit_width());
        let target_type = cx.context.custom_width_int_type(target_bits);
        let li = if li.get_type().get_bit_width() < target_bits {
            cx.builder
                .build_int_s_extend(li, target_type, "sext")
                .expect("build_int_s_extend failed")
        } else {
            li
        };
        let ri = if ri.get_type().get_bit_width() < target_bits {
            cx.builder
                .build_int_s_extend(ri, target_type, "sext")
                .expect("build_int_s_extend failed")
        } else {
            ri
        };
        (li, ri)
    } else {
        (li, ri)
    };

    let result: BasicValueEnum = match op {
        BinOp::Add => cx
            .builder
            .build_int_add(li, ri, "iadd")
            .expect("build_int_add failed")
            .into(),
        BinOp::Sub => cx
            .builder
            .build_int_sub(li, ri, "isub")
            .expect("build_int_sub failed")
            .into(),
        BinOp::Mul => cx
            .builder
            .build_int_mul(li, ri, "imul")
            .expect("build_int_mul failed")
            .into(),
        BinOp::Div => {
            emit_div_zero_check(cx, ri);
            cx.builder
                .build_int_signed_div(li, ri, "sdiv")
                .expect("build_int_signed_div failed")
                .into()
        }
        BinOp::Mod => {
            emit_div_zero_check(cx, ri);
            cx.builder
                .build_int_signed_rem(li, ri, "srem")
                .expect("build_int_signed_rem failed")
                .into()
        }
        BinOp::Eq => cx
            .builder
            .build_int_compare(IntPredicate::EQ, li, ri, "ieq")
            .expect("build_int_compare failed")
            .into(),
        BinOp::NotEq => cx
            .builder
            .build_int_compare(IntPredicate::NE, li, ri, "ineq")
            .expect("build_int_compare failed")
            .into(),
        BinOp::Less => cx
            .builder
            .build_int_compare(IntPredicate::SLT, li, ri, "ilt")
            .expect("build_int_compare failed")
            .into(),
        BinOp::LessEq => cx
            .builder
            .build_int_compare(IntPredicate::SLE, li, ri, "ile")
            .expect("build_int_compare failed")
            .into(),
        BinOp::Greater => cx
            .builder
            .build_int_compare(IntPredicate::SGT, li, ri, "igt")
            .expect("build_int_compare failed")
            .into(),
        BinOp::GreaterEq => cx
            .builder
            .build_int_compare(IntPredicate::SGE, li, ri, "ige")
            .expect("build_int_compare failed")
            .into(),
        BinOp::And => cx
            .builder
            .build_and(li, ri, "and")
            .expect("build_and failed")
            .into(),
        BinOp::Or => cx
            .builder
            .build_or(li, ri, "or")
            .expect("build_or failed")
            .into(),
    };
    // Widen i1 comparison results to i8 for consistent Bool representation
    let result = widen_i1_to_i8(cx, result);
    Ok(result)
}

/// If a value is i1 (LLVM comparison result), widen it to i8 (Turbo Bool type).
fn widen_i1_to_i8<'a, 'ctx>(cx: &Ctx<'a, 'ctx>, val: BasicValueEnum<'ctx>) -> BasicValueEnum<'ctx> {
    if let BasicValueEnum::IntValue(iv) = val {
        if iv.get_type().get_bit_width() == 1 {
            return cx
                .builder
                .build_int_z_extend(iv, cx.context.i8_type(), "i1_to_i8")
                .expect("build_int_z_extend failed")
                .into();
        }
    }
    val
}

fn emit_div_zero_check<'a, 'ctx>(cx: &mut Ctx<'a, 'ctx>, divisor: IntValue<'ctx>) {
    let zero = divisor.get_type().const_int(0, false);
    let is_zero = cx
        .builder
        .build_int_compare(IntPredicate::EQ, divisor, zero, "divzero")
        .expect("build_int_compare failed");

    let trap_block = cx.context.append_basic_block(cx.current_fn, "div_trap");
    let ok_block = cx.context.append_basic_block(cx.current_fn, "div_ok");

    cx.builder
        .build_conditional_branch(is_zero, trap_block, ok_block)
        .expect("build_conditional_branch failed");

    cx.builder.position_at_end(trap_block);
    cx.rt_call("rt_div_by_zero", &[]);
    cx.builder
        .build_unreachable()
        .expect("build_unreachable failed");

    cx.builder.position_at_end(ok_block);
}

// ── Short-circuit && / || ───────────────────────────────────────────

fn compile_short_circuit<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    left: &Spanned<Expr>,
    op: BinOp,
    right: &Spanned<Expr>,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    let (lhs, _) = compile_expr(cx, left)?.unwrap();
    let lhs_bool = cx.to_bool(lhs);

    let eval_rhs_block = cx.context.append_basic_block(cx.current_fn, "sc_rhs");
    let merge_block = cx.context.append_basic_block(cx.current_fn, "sc_merge");

    let current_block = cx.builder.get_insert_block().unwrap();

    match op {
        BinOp::And => {
            cx.builder
                .build_conditional_branch(lhs_bool, eval_rhs_block, merge_block)
                .expect("build_conditional_branch failed");
        }
        BinOp::Or => {
            cx.builder
                .build_conditional_branch(lhs_bool, merge_block, eval_rhs_block)
                .expect("build_conditional_branch failed");
        }
        _ => unreachable!(),
    }

    cx.builder.position_at_end(eval_rhs_block);
    let (rhs, _) = compile_expr(cx, right)?.unwrap();
    let rhs_bool = cx.to_bool(rhs);
    let rhs_end_block = cx.builder.get_insert_block().unwrap();
    cx.builder
        .build_unconditional_branch(merge_block)
        .expect("build_unconditional_branch failed");

    cx.builder.position_at_end(merge_block);
    let phi = cx
        .builder
        .build_phi(cx.context.bool_type(), "sc_result")
        .expect("build_phi failed");

    match op {
        BinOp::And => {
            let false_val = cx.context.bool_type().const_int(0, false);
            phi.add_incoming(&[(&false_val, current_block), (&rhs_bool, rhs_end_block)]);
        }
        BinOp::Or => {
            let true_val = cx.context.bool_type().const_int(1, false);
            phi.add_incoming(&[(&true_val, current_block), (&rhs_bool, rhs_end_block)]);
        }
        _ => unreachable!(),
    }

    // Widen i1 back to i8 for consistent Bool representation
    let result = cx
        .builder
        .build_int_z_extend(
            phi.as_basic_value().into_int_value(),
            cx.context.i8_type(),
            "sc_zext",
        )
        .expect("build_int_z_extend failed");
    Ok(Some((result.into(), TurboTy::Bool)))
}

// ── If/else ─────────────────────────────────────────────────────────

fn compile_if<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    condition: &Spanned<Expr>,
    then_branch: &Spanned<Expr>,
    else_branch: Option<&Spanned<Expr>>,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    let (cond, _) = compile_expr(cx, condition)?.unwrap();
    let cond_bool = cx.to_bool(cond);

    let then_block = cx.context.append_basic_block(cx.current_fn, "then");
    let else_block = cx.context.append_basic_block(cx.current_fn, "else");
    let merge_block = cx.context.append_basic_block(cx.current_fn, "ifmerge");

    cx.builder
        .build_conditional_branch(cond_bool, then_block, else_block)
        .expect("build_conditional_branch failed");

    // Then branch
    cx.builder.position_at_end(then_block);
    let then_result = compile_expr(cx, then_branch)?;
    let then_end_block = cx.builder.get_insert_block().unwrap();
    let then_needs_jump = then_end_block.get_terminator().is_none();
    if then_needs_jump {
        cx.builder
            .build_unconditional_branch(merge_block)
            .expect("build_unconditional_branch failed");
    }

    // Else branch
    cx.builder.position_at_end(else_block);
    let else_result = if let Some(else_expr) = else_branch {
        compile_expr(cx, else_expr)?
    } else {
        None
    };
    let else_end_block = cx.builder.get_insert_block().unwrap();
    let else_needs_jump = else_end_block.get_terminator().is_none();
    if else_needs_jump {
        cx.builder
            .build_unconditional_branch(merge_block)
            .expect("build_unconditional_branch failed");
    }

    // Merge block
    cx.builder.position_at_end(merge_block);

    if let (Some((then_val, then_tty)), Some((else_val, _))) = (then_result, else_result) {
        if then_needs_jump && else_needs_jump {
            let phi = cx
                .builder
                .build_phi(then_val.get_type(), "ifphi")
                .expect("build_phi failed");
            phi.add_incoming(&[(&then_val, then_end_block), (&else_val, else_end_block)]);
            Ok(Some((phi.as_basic_value(), then_tty)))
        } else if then_needs_jump {
            // Only then branch reaches merge
            Ok(Some((then_val, then_tty)))
        } else if else_needs_jump {
            // Only else branch reaches merge
            Ok(Some((else_val, then_tty)))
        } else {
            // Neither branch reaches merge (both return/break)
            Ok(None)
        }
    } else {
        Ok(None)
    }
}

// ── While loop ──────────────────────────────────────────────────────

fn compile_while<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    condition: &Spanned<Expr>,
    body: &Spanned<Expr>,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    let header_block = cx.context.append_basic_block(cx.current_fn, "while_header");
    let body_block = cx.context.append_basic_block(cx.current_fn, "while_body");
    let exit_block = cx.context.append_basic_block(cx.current_fn, "while_exit");

    cx.builder
        .build_unconditional_branch(header_block)
        .expect("build_unconditional_branch failed");

    cx.builder.position_at_end(header_block);
    let (cond, _) = compile_expr(cx, condition)?.unwrap();
    let cond_bool = cx.to_bool(cond);
    cx.builder
        .build_conditional_branch(cond_bool, body_block, exit_block)
        .expect("build_conditional_branch failed");

    cx.builder.position_at_end(body_block);
    cx.loop_stack.push((header_block, exit_block));
    compile_expr(cx, body)?;
    cx.loop_stack.pop();

    let body_end = cx.builder.get_insert_block().unwrap();
    if body_end.get_terminator().is_none() {
        cx.builder
            .build_unconditional_branch(header_block)
            .expect("build_unconditional_branch failed");
    }

    cx.builder.position_at_end(exit_block);
    Ok(None)
}

// ── For-in loop ─────────────────────────────────────────────────────

fn compile_for_in<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    var_name: &str,
    iterable: &Spanned<Expr>,
    body: &Spanned<Expr>,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    match &iterable.node {
        Expr::Range { start, end } => compile_for_in_range(cx, var_name, start, end, body),
        _ => compile_for_in_array(cx, var_name, iterable, body),
    }
}

fn compile_for_in_range<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    var_name: &str,
    start: &Spanned<Expr>,
    end: &Spanned<Expr>,
    body: &Spanned<Expr>,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    let (range_start, _) = compile_expr(cx, start)?.unwrap();
    let (range_end, _) = compile_expr(cx, end)?.unwrap();

    let alloca = cx.create_entry_block_alloca(cx.context.i64_type().into(), var_name);
    cx.builder
        .build_store(alloca, range_start)
        .expect("build_store failed");
    cx.vars.insert(var_name.to_string(), (alloca, TurboTy::Int));

    let header_block = cx.context.append_basic_block(cx.current_fn, "forin_header");
    let body_block = cx.context.append_basic_block(cx.current_fn, "forin_body");
    let continue_block = cx
        .context
        .append_basic_block(cx.current_fn, "forin_continue");
    let exit_block = cx.context.append_basic_block(cx.current_fn, "forin_exit");

    cx.builder
        .build_unconditional_branch(header_block)
        .expect("build_unconditional_branch failed");

    cx.builder.position_at_end(header_block);
    let current_i = cx
        .builder
        .build_load(cx.context.i64_type(), alloca, "i")
        .expect("build_load failed")
        .into_int_value();
    let range_end_i = range_end.into_int_value();
    let cond = cx
        .builder
        .build_int_compare(IntPredicate::SLT, current_i, range_end_i, "forin_cond")
        .expect("build_int_compare failed");
    cx.builder
        .build_conditional_branch(cond, body_block, exit_block)
        .expect("build_conditional_branch failed");

    cx.builder.position_at_end(body_block);
    cx.loop_stack.push((continue_block, exit_block));
    compile_expr(cx, body)?;
    cx.loop_stack.pop();

    let body_end = cx.builder.get_insert_block().unwrap();
    if body_end.get_terminator().is_none() {
        cx.builder
            .build_unconditional_branch(continue_block)
            .expect("build_unconditional_branch failed");
    }

    cx.builder.position_at_end(continue_block);
    let updated_i = cx
        .builder
        .build_load(cx.context.i64_type(), alloca, "i_cur")
        .expect("build_load failed")
        .into_int_value();
    let one = cx.context.i64_type().const_int(1, false);
    let next_i = cx
        .builder
        .build_int_add(updated_i, one, "next_i")
        .expect("build_int_add failed");
    cx.builder
        .build_store(alloca, next_i)
        .expect("build_store failed");
    cx.builder
        .build_unconditional_branch(header_block)
        .expect("build_unconditional_branch failed");

    cx.builder.position_at_end(exit_block);
    Ok(None)
}

fn compile_for_in_array<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    var_name: &str,
    iterable: &Spanned<Expr>,
    body: &Spanned<Expr>,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    let (arr, arr_tty) = compile_expr(cx, iterable)?.unwrap();
    let elem_tty = match &arr_tty {
        TurboTy::Array(inner) => *inner.clone(),
        _ => TurboTy::Int,
    };

    let arr_len = cx
        .rt_call("rt_array_len", &[arr.into()])
        .unwrap()
        .into_int_value();

    // Index counter
    let idx_alloca = cx.create_entry_block_alloca(cx.context.i64_type().into(), "__forin_idx");
    cx.builder
        .build_store(idx_alloca, cx.context.i64_type().const_int(0, false))
        .expect("build_store failed");

    // Loop variable
    let elem_llvm_ty = turbo_ty_to_llvm_ctx(&elem_tty, cx.context, cx.enum_max_slots);
    let var_alloca = cx.create_entry_block_alloca(elem_llvm_ty, var_name);
    cx.vars
        .insert(var_name.to_string(), (var_alloca, elem_tty.clone()));

    let header_block = cx
        .context
        .append_basic_block(cx.current_fn, "forin_arr_header");
    let body_block = cx
        .context
        .append_basic_block(cx.current_fn, "forin_arr_body");
    let continue_block = cx
        .context
        .append_basic_block(cx.current_fn, "forin_arr_continue");
    let exit_block = cx
        .context
        .append_basic_block(cx.current_fn, "forin_arr_exit");

    cx.builder
        .build_unconditional_branch(header_block)
        .expect("build_unconditional_branch failed");

    cx.builder.position_at_end(header_block);
    let idx = cx
        .builder
        .build_load(cx.context.i64_type(), idx_alloca, "idx")
        .expect("build_load failed")
        .into_int_value();
    let cond = cx
        .builder
        .build_int_compare(IntPredicate::SLT, idx, arr_len, "forin_arr_cond")
        .expect("build_int_compare failed");
    cx.builder
        .build_conditional_branch(cond, body_block, exit_block)
        .expect("build_conditional_branch failed");

    cx.builder.position_at_end(body_block);
    // Load element
    let idx2 = cx
        .builder
        .build_load(cx.context.i64_type(), idx_alloca, "idx")
        .expect("build_load failed");
    let raw_elem = cx
        .rt_call("rt_array_get", &[arr.into(), idx2.into()])
        .unwrap();
    let elem = narrow_from_storage(cx, raw_elem, &elem_tty);
    cx.builder
        .build_store(var_alloca, elem)
        .expect("build_store failed");

    cx.loop_stack.push((continue_block, exit_block));
    compile_expr(cx, body)?;
    cx.loop_stack.pop();

    let body_end = cx.builder.get_insert_block().unwrap();
    if body_end.get_terminator().is_none() {
        cx.builder
            .build_unconditional_branch(continue_block)
            .expect("build_unconditional_branch failed");
    }

    cx.builder.position_at_end(continue_block);
    let idx3 = cx
        .builder
        .build_load(cx.context.i64_type(), idx_alloca, "idx")
        .expect("build_load failed")
        .into_int_value();
    let one = cx.context.i64_type().const_int(1, false);
    let next = cx
        .builder
        .build_int_add(idx3, one, "next_idx")
        .expect("build_int_add failed");
    cx.builder
        .build_store(idx_alloca, next)
        .expect("build_store failed");
    cx.builder
        .build_unconditional_branch(header_block)
        .expect("build_unconditional_branch failed");

    cx.builder.position_at_end(exit_block);
    Ok(None)
}

// ── Match expression ────────────────────────────────────────────────

fn compile_match<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    subject: &Spanned<Expr>,
    arms: &[MatchArm],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    let (subject_val, subject_tty) = compile_expr(cx, subject)?.unwrap();

    let merge_block = cx.context.append_basic_block(cx.current_fn, "match_merge");

    let mut arm_blocks: Vec<inkwell::basic_block::BasicBlock<'ctx>> = Vec::new();
    for i in 0..arms.len() {
        arm_blocks.push(
            cx.context
                .append_basic_block(cx.current_fn, &format!("match_arm_{i}")),
        );
    }
    let default_block = cx
        .context
        .append_basic_block(cx.current_fn, "match_default");

    // Build chain of comparisons
    let mut phi_incoming: Vec<(BasicValueEnum<'ctx>, inkwell::basic_block::BasicBlock<'ctx>)> =
        Vec::new();
    let mut first_arm_tty: Option<TurboTy> = None;

    for (i, arm) in arms.iter().enumerate() {
        let arm_block = arm_blocks[i];
        let next_block = if i + 1 < arms.len() {
            cx.context
                .append_basic_block(cx.current_fn, &format!("match_test_{}", i + 1))
        } else {
            default_block
        };

        // Only branch from the first test block for the first arm
        if i == 0 {
            // We're still at the end of the block before the match
        }

        // Build test
        let matches = match &arm.pattern.node {
            Pattern::Wildcard => None,
            Pattern::Ident(name) => {
                // Check if this ident is an enum variant name
                let variant_tag = lookup_variant_tag(cx.enum_variants, name);
                if let Some(tag_val) = variant_tag {
                    let pat_val = cx.context.i64_type().const_int(tag_val as u64, false);
                    if let TurboTy::Enum(ref enum_name) = subject_tty {
                        if cx.enum_max_slots.contains_key(enum_name) {
                            // Data enum: load tag from ptr
                            let ptr = subject_val.into_pointer_value();
                            let tag = cx
                                .builder
                                .build_load(cx.context.i64_type(), ptr, "tag")
                                .expect("build_load failed")
                                .into_int_value();
                            Some(
                                cx.builder
                                    .build_int_compare(IntPredicate::EQ, tag, pat_val, "var_eq")
                                    .expect("build_int_compare failed"),
                            )
                        } else {
                            // Unit enum: direct tag compare
                            let tag = subject_val.into_int_value();
                            Some(
                                cx.builder
                                    .build_int_compare(IntPredicate::EQ, tag, pat_val, "var_eq")
                                    .expect("build_int_compare failed"),
                            )
                        }
                    } else {
                        // Subject is int, compare directly
                        let tag = subject_val.into_int_value();
                        Some(
                            cx.builder
                                .build_int_compare(IntPredicate::EQ, tag, pat_val, "var_eq")
                                .expect("build_int_compare failed"),
                        )
                    }
                } else {
                    None // catch-all bind
                }
            }
            Pattern::IntLit(n) => {
                let pat_val = cx.context.i64_type().const_int(*n as u64, true);
                let subject_int = subject_val.into_int_value();
                Some(
                    cx.builder
                        .build_int_compare(IntPredicate::EQ, subject_int, pat_val, "pat_eq")
                        .expect("build_int_compare failed"),
                )
            }
            Pattern::BoolLit(b) => {
                let pat_val = cx.context.i8_type().const_int(*b as u64, false);
                let subject_int = subject_val.into_int_value();
                let subject_i8 = if subject_int.get_type().get_bit_width() > 8 {
                    cx.builder
                        .build_int_truncate(subject_int, cx.context.i8_type(), "trunc")
                        .expect("build_int_truncate failed")
                } else {
                    subject_int
                };
                Some(
                    cx.builder
                        .build_int_compare(IntPredicate::EQ, subject_i8, pat_val, "pat_eq")
                        .expect("build_int_compare failed"),
                )
            }
            Pattern::StringLit(s) => {
                let pat_ptr = cx.create_string(s)?;
                let eq = cx
                    .rt_call("rt_str_eq", &[subject_val.into(), pat_ptr.into()])
                    .unwrap()
                    .into_int_value();
                let zero = cx.context.i8_type().const_int(0, false);
                Some(
                    cx.builder
                        .build_int_compare(IntPredicate::NE, eq, zero, "pat_eq")
                        .expect("build_int_compare failed"),
                )
            }
            Pattern::Ok(_) => {
                // Result tag 0 = Ok
                let tag = cx
                    .rt_call("rt_result_tag", &[subject_val.into()])
                    .unwrap()
                    .into_int_value();
                let zero = cx.context.i64_type().const_int(0, false);
                Some(
                    cx.builder
                        .build_int_compare(IntPredicate::EQ, tag, zero, "is_ok")
                        .expect("build_int_compare failed"),
                )
            }
            Pattern::Err(_) => {
                let tag = cx
                    .rt_call("rt_result_tag", &[subject_val.into()])
                    .unwrap()
                    .into_int_value();
                let one = cx.context.i64_type().const_int(1, false);
                Some(
                    cx.builder
                        .build_int_compare(IntPredicate::EQ, tag, one, "is_err")
                        .expect("build_int_compare failed"),
                )
            }
            Pattern::Some(_) => {
                let tag = cx
                    .rt_call("rt_option_tag", &[subject_val.into()])
                    .unwrap()
                    .into_int_value();
                let one = cx.context.i64_type().const_int(1, false);
                Some(
                    cx.builder
                        .build_int_compare(IntPredicate::EQ, tag, one, "is_some")
                        .expect("build_int_compare failed"),
                )
            }
            Pattern::None => {
                let tag = cx
                    .rt_call("rt_option_tag", &[subject_val.into()])
                    .unwrap()
                    .into_int_value();
                let zero = cx.context.i64_type().const_int(0, false);
                Some(
                    cx.builder
                        .build_int_compare(IntPredicate::EQ, tag, zero, "is_none")
                        .expect("build_int_compare failed"),
                )
            }
            Pattern::VariantDestructure { variant, .. } => {
                // Match on enum tag
                if let TurboTy::Enum(ref enum_name) = subject_tty {
                    if let Some(variants) = cx.enum_variants.get(enum_name) {
                        if let Some(idx) = variants.iter().position(|v| v == variant) {
                            let tag_val = cx.context.i64_type().const_int(idx as u64, false);
                            // For data enums, load tag from heap
                            if cx.enum_max_slots.contains_key(enum_name) {
                                let ptr = subject_val.into_pointer_value();
                                let tag = cx
                                    .builder
                                    .build_load(cx.context.i64_type(), ptr, "tag")
                                    .expect("build_load failed")
                                    .into_int_value();
                                Some(
                                    cx.builder
                                        .build_int_compare(IntPredicate::EQ, tag, tag_val, "var_eq")
                                        .expect("build_int_compare failed"),
                                )
                            } else {
                                let tag = subject_val.into_int_value();
                                Some(
                                    cx.builder
                                        .build_int_compare(IntPredicate::EQ, tag, tag_val, "var_eq")
                                        .expect("build_int_compare failed"),
                                )
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        };

        let has_pattern_test = matches.is_some();
        if let Some(cond) = matches {
            if arm.guard.is_some() {
                // Pattern matched — jump to a guard-check block
                let guard_block = cx
                    .context
                    .append_basic_block(cx.current_fn, &format!("match_guard_{i}"));
                cx.builder
                    .build_conditional_branch(cond, guard_block, next_block)
                    .expect("build_conditional_branch failed");
                cx.builder.position_at_end(guard_block);
            } else {
                cx.builder
                    .build_conditional_branch(cond, arm_block, next_block)
                    .expect("build_conditional_branch failed");
            }
        } else {
            // Wildcard or Ident: always matches
            if arm.guard.is_some() {
                // Need a guard block for wildcard + guard
                let guard_block = cx
                    .context
                    .append_basic_block(cx.current_fn, &format!("match_guard_{i}"));
                cx.builder
                    .build_unconditional_branch(guard_block)
                    .expect("br");
                cx.builder.position_at_end(guard_block);
            } else {
                cx.builder
                    .build_unconditional_branch(arm_block)
                    .expect("build_unconditional_branch failed");
            }
        }

        // If there's a guard, we're now in the guard block.
        // Bind pattern variables first (guard may reference them), then evaluate guard.
        if arm.guard.is_some() {
            // We're in the guard_block; bind vars, eval guard, branch
            // Bind variables needed by guard
            // (For simplicity, bind subject for ident patterns)
            let saved_guard_vars = cx.vars.clone();
            match &arm.pattern.node {
                Pattern::Ident(name)
                    if name != "_" && lookup_variant_tag(cx.enum_variants, name).is_none() =>
                {
                    let llvm_ty = subject_val.get_type();
                    let alloca = cx.create_entry_block_alloca(llvm_ty, name);
                    cx.builder.build_store(alloca, subject_val).expect("store");
                    cx.vars.insert(name.clone(), (alloca, subject_tty.clone()));
                }
                Pattern::VariantDestructure { variant, bindings } => {
                    if let TurboTy::Enum(ref enum_name) = subject_tty {
                        if cx.enum_max_slots.contains_key(enum_name) {
                            let ptr = subject_val.into_pointer_value();
                            for (j, bname) in bindings.iter().enumerate() {
                                if bname == "_" {
                                    continue;
                                }
                                let offset = ((j + 1) * 8) as u64;
                                let field_ptr = unsafe {
                                    cx.builder
                                        .build_gep(
                                            cx.context.i8_type(),
                                            ptr,
                                            &[cx.context.i64_type().const_int(offset, false)],
                                            "guard_bind_ptr",
                                        )
                                        .expect("gep")
                                };
                                let val = cx
                                    .builder
                                    .build_load(cx.context.i64_type(), field_ptr, "guard_bind_val")
                                    .expect("load");
                                let field_tty = cx
                                    .enum_variant_fields
                                    .get(&(enum_name.clone(), variant.clone()))
                                    .and_then(|fs| fs.get(j))
                                    .cloned()
                                    .unwrap_or(TurboTy::Int);
                                let alloca = cx.create_entry_block_alloca(val.get_type(), bname);
                                cx.builder.build_store(alloca, val).expect("store");
                                cx.vars.insert(bname.clone(), (alloca, field_tty));
                            }
                        }
                    }
                }
                _ => {}
            }
            let guard_expr = arm.guard.as_ref().unwrap();
            let (guard_val, _) = compile_expr(cx, guard_expr)?.unwrap();
            let guard_bool = guard_val.into_int_value();
            // Normalize to i1 for the branch
            let guard_cond = if guard_bool.get_type().get_bit_width() == 1 {
                guard_bool
            } else {
                let zero = guard_bool.get_type().const_int(0, false);
                cx.builder
                    .build_int_compare(IntPredicate::NE, guard_bool, zero, "guard_cond")
                    .expect("icmp")
            };
            cx.builder
                .build_conditional_branch(guard_cond, arm_block, next_block)
                .expect("cond_br");
            cx.vars = saved_guard_vars;
        }

        // Compile arm body
        cx.builder.position_at_end(arm_block);

        // Bind pattern variables
        let saved_vars = cx.vars.clone();
        match &arm.pattern.node {
            Pattern::Ident(name)
                if name != "_" && lookup_variant_tag(cx.enum_variants, name).is_none() =>
            {
                // Catch-all bind: bind subject to name
                let llvm_ty = subject_val.get_type();
                let alloca = cx.create_entry_block_alloca(llvm_ty, name);
                cx.builder
                    .build_store(alloca, subject_val)
                    .expect("build_store failed");
                cx.vars.insert(name.clone(), (alloca, subject_tty.clone()));
            }
            Pattern::Ok(name) | Pattern::Some(name) => {
                let is_ok = matches!(arm.pattern.node, Pattern::Ok(_));
                let inner_tty = match &subject_tty {
                    TurboTy::Result(ok_ty, _) if is_ok => *ok_ty.clone(),
                    TurboTy::Optional(inner_ty) if !is_ok => *inner_ty.clone(),
                    _ => TurboTy::Int,
                };
                let inner_raw = if is_ok {
                    cx.rt_call("rt_result_value", &[subject_val.into()])
                        .unwrap()
                } else {
                    cx.rt_call("rt_option_value", &[subject_val.into()])
                        .unwrap()
                };
                // inner_raw is i64; narrow to the inner type for storage
                let inner_narrowed = narrow_from_storage(cx, inner_raw, &inner_tty);
                let inner_llvm_ty = turbo_ty_to_llvm_ctx(&inner_tty, cx.context, cx.enum_max_slots);
                let alloca = cx.create_entry_block_alloca(inner_llvm_ty, name);
                cx.builder
                    .build_store(alloca, inner_narrowed)
                    .expect("build_store failed");
                cx.vars.insert(name.clone(), (alloca, inner_tty));
            }
            Pattern::Err(name) => {
                let inner_tty = match &subject_tty {
                    TurboTy::Result(_, err_ty) => *err_ty.clone(),
                    _ => TurboTy::Int,
                };
                let inner_raw = cx
                    .rt_call("rt_result_value", &[subject_val.into()])
                    .unwrap();
                let inner_narrowed = narrow_from_storage(cx, inner_raw, &inner_tty);
                let inner_llvm_ty = turbo_ty_to_llvm_ctx(&inner_tty, cx.context, cx.enum_max_slots);
                let alloca = cx.create_entry_block_alloca(inner_llvm_ty, name);
                cx.builder
                    .build_store(alloca, inner_narrowed)
                    .expect("build_store failed");
                cx.vars.insert(name.clone(), (alloca, inner_tty));
            }
            Pattern::VariantDestructure { variant, bindings } => {
                // Bind destructured fields
                if let TurboTy::Enum(ref enum_name) = subject_tty {
                    if cx.enum_max_slots.contains_key(enum_name) {
                        let ptr = subject_val.into_pointer_value();
                        for (j, binding_name) in bindings.iter().enumerate() {
                            if binding_name == "_" {
                                continue;
                            }
                            let offset = ((j + 1) * 8) as u64;
                            let field_ptr = unsafe {
                                cx.builder
                                    .build_gep(
                                        cx.context.i8_type(),
                                        ptr,
                                        &[cx.context.i64_type().const_int(offset, false)],
                                        "vf_ptr",
                                    )
                                    .expect("build_gep failed")
                            };
                            let field_val = cx
                                .builder
                                .build_load(cx.context.i64_type(), field_ptr, binding_name)
                                .expect("build_load failed");
                            // Determine field type
                            let field_tty = cx
                                .enum_variant_fields
                                .get(&(enum_name.clone(), variant.clone()))
                                .and_then(|tys| tys.get(j).cloned())
                                .unwrap_or(TurboTy::Int);
                            let field_val = narrow_from_storage(cx, field_val, &field_tty);
                            let alloca =
                                cx.create_entry_block_alloca(field_val.get_type(), binding_name);
                            cx.builder
                                .build_store(alloca, field_val)
                                .expect("build_store failed");
                            cx.vars.insert(binding_name.clone(), (alloca, field_tty));
                        }
                    }
                }
            }
            _ => {}
        }

        let arm_result = compile_expr(cx, &arm.body)?;
        cx.vars = saved_vars;

        let arm_end_block = cx.builder.get_insert_block().unwrap();
        if arm_end_block.get_terminator().is_none() {
            if let Some((val, ref tty)) = arm_result {
                if first_arm_tty.is_none() {
                    first_arm_tty = Some(tty.clone());
                }
                phi_incoming.push((val, arm_end_block));
            }
            cx.builder
                .build_unconditional_branch(merge_block)
                .expect("build_unconditional_branch failed");
        }

        // Position at next test block if needed
        if i + 1 < arms.len() {
            cx.builder.position_at_end(next_block);
        }
    }

    // Default block (unreachable)
    cx.builder.position_at_end(default_block);
    cx.builder
        .build_unreachable()
        .expect("build_unreachable failed");

    // Merge
    cx.builder.position_at_end(merge_block);

    if !phi_incoming.is_empty() && first_arm_tty.is_some() {
        let first_type = phi_incoming[0].0.get_type();
        let phi = cx
            .builder
            .build_phi(first_type, "match_result")
            .expect("build_phi failed");
        for (val, block) in &phi_incoming {
            phi.add_incoming(&[(val, *block)]);
        }
        Ok(Some((phi.as_basic_value(), first_arm_tty.unwrap())))
    } else {
        Ok(None)
    }
}

// ── Function calls ──────────────────────────────────────────────────

fn compile_call<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    callee: &Spanned<Expr>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    // Method calls: expr.method(args)
    if let Expr::FieldAccess {
        ref object,
        ref field,
    } = callee.node
    {
        let (obj_val, obj_tty) = compile_expr(cx, object)?.unwrap();
        if let TurboTy::Struct(ref type_name) = obj_tty {
            let mangled = format!("{}__{}", type_name, field);
            if let Some(&func) = cx.user_fns.get(&mangled) {
                let mut arg_vals: Vec<BasicMetadataValueEnum> = vec![obj_val.into()];
                for arg in args {
                    if let Some((v, _)) = compile_expr(cx, arg)? {
                        arg_vals.push(v.into());
                    }
                }
                let call = cx
                    .builder
                    .build_direct_call(func, &arg_vals, "")
                    .expect("build_direct_call failed");
                let ret_tty = cx
                    .fn_ret_types
                    .get(&mangled)
                    .cloned()
                    .unwrap_or(TurboTy::Unit);
                return match call.try_as_basic_value().left() {
                    Some(val) => Ok(Some((val, ret_tty))),
                    None => Ok(None),
                };
            }
        }
        // String/array method calls
        return compile_method_call(cx, obj_val, &obj_tty, field, args);
    }

    // Check if callee is a closure variable (TurboTy::Fn stored in vars)
    if let Expr::Ident(callee_name) = &callee.node {
        if let Some((alloca, TurboTy::Fn(ref param_tys, ref ret_ty))) =
            cx.vars.get(callee_name.as_str()).cloned()
        {
            let param_tys = param_tys.clone();
            let ret_ty = *ret_ty.clone();
            let ptr_type = cx.context.ptr_type(AddressSpace::default());
            let i8_type = cx.context.i8_type();
            let i64_type = cx.context.i64_type();

            // Load the closure pair struct pointer
            let closure_ptr = cx
                .builder
                .build_load(ptr_type, alloca, "closure_ptr")
                .expect("load")
                .into_pointer_value();

            // Load fn_ptr (as i64) from offset 0, convert to pointer
            let fn_ptr_i64 = cx
                .builder
                .build_load(i64_type, closure_ptr, "fn_ptr_i64")
                .expect("load")
                .into_int_value();
            let fn_ptr = cx
                .builder
                .build_int_to_ptr(fn_ptr_i64, ptr_type, "fn_ptr")
                .expect("itp");

            // Load env_ptr (as i64) from offset 8, convert to pointer
            let env_slot = unsafe {
                cx.builder
                    .build_gep(
                        i8_type,
                        closure_ptr,
                        &[i64_type.const_int(8, false)],
                        "env_slot",
                    )
                    .expect("gep")
            };
            let env_ptr_i64 = cx
                .builder
                .build_load(i64_type, env_slot, "env_ptr_i64")
                .expect("load")
                .into_int_value();
            let env_ptr = cx
                .builder
                .build_int_to_ptr(env_ptr_i64, ptr_type, "env_ptr")
                .expect("itp");

            // Build LLVM function type: (ptr, ...params) -> ret
            let mut llvm_param_types: Vec<BasicMetadataTypeEnum<'ctx>> = vec![ptr_type.into()]; // env_ptr
            for pt in &param_tys {
                llvm_param_types
                    .push(turbo_ty_to_llvm_ctx(pt, cx.context, cx.enum_max_slots).into());
            }
            let fn_type = if ret_ty == TurboTy::Unit {
                cx.context.void_type().fn_type(&llvm_param_types, false)
            } else {
                let ret_llvm = turbo_ty_to_llvm_ctx(&ret_ty, cx.context, cx.enum_max_slots);
                ret_llvm.fn_type(&llvm_param_types, false)
            };

            // Compile arguments
            let mut arg_vals: Vec<BasicMetadataValueEnum<'ctx>> = vec![env_ptr.into()];
            for (i, arg) in args.iter().enumerate() {
                if let Some((val, _)) = compile_expr(cx, arg)? {
                    if i < param_tys.len() {
                        let expected =
                            turbo_ty_to_llvm_ctx(&param_tys[i], cx.context, cx.enum_max_slots);
                        let val = coerce_arg(cx, val, expected);
                        arg_vals.push(val.into());
                    } else {
                        arg_vals.push(val.into());
                    }
                }
            }

            let call = cx
                .builder
                .build_indirect_call(fn_type, fn_ptr, &arg_vals, "closure_call")
                .expect("indirect call failed");

            return match call.try_as_basic_value().left() {
                Some(val) => Ok(Some((val, ret_ty))),
                None => Ok(None),
            };
        }
    }

    let Expr::Ident(name) = &callee.node else {
        return Err(CodegenError {
            code: ErrorCode::E0400,
            message: "indirect function calls not yet supported in LLVM backend".to_string(),
        });
    };

    match name.as_str() {
        "print" => compile_print(cx, args),
        "panic" => compile_panic(cx, args),
        "assert" => compile_assert(cx, args),
        "assert_eq" => compile_assert_eq(cx, args, false),
        "assert_ne" => compile_assert_eq(cx, args, true),
        "len" => compile_len(cx, args),
        "abs" => compile_abs(cx, args),
        "min" => compile_min_max(cx, args, true),
        "max" => compile_min_max(cx, args, false),
        "to_str" => compile_to_str_builtin(cx, args),
        // Stdlib
        "split" => compile_stdlib_2arg_rt(
            cx,
            args,
            "rt_str_split",
            TurboTy::Array(Box::new(TurboTy::Str)),
        ),
        "trim" => compile_stdlib_1arg_rt(cx, args, "rt_str_trim", TurboTy::Str),
        "upper" => compile_stdlib_1arg_rt(cx, args, "rt_str_upper", TurboTy::Str),
        "lower" => compile_stdlib_1arg_rt(cx, args, "rt_str_lower", TurboTy::Str),
        "starts_with" => compile_stdlib_2arg_rt(cx, args, "rt_str_starts_with", TurboTy::Bool),
        "ends_with" => compile_stdlib_2arg_rt(cx, args, "rt_str_ends_with", TurboTy::Bool),
        "contains" => compile_stdlib_2arg_rt(cx, args, "rt_str_contains", TurboTy::Bool),
        "index_of" => compile_stdlib_2arg_rt(cx, args, "rt_str_index_of", TurboTy::Int),
        "replace" => {
            if args.len() >= 3 {
                let (a, _) = compile_expr(cx, &args[0])?.unwrap();
                let (b, _) = compile_expr(cx, &args[1])?.unwrap();
                let (c, _) = compile_expr(cx, &args[2])?.unwrap();
                let result = cx
                    .rt_call("rt_str_replace", &[a.into(), b.into(), c.into()])
                    .unwrap();
                Ok(Some((result, TurboTy::Str)))
            } else {
                Ok(None)
            }
        }
        "char_at" => compile_stdlib_2arg_rt(cx, args, "rt_str_char_at", TurboTy::Str),
        "join" => compile_stdlib_2arg_rt(cx, args, "rt_str_join", TurboTy::Str),
        "repeat" => compile_stdlib_2arg_rt(cx, args, "rt_str_repeat", TurboTy::Str),
        "read_line" => {
            let result = cx.rt_call("rt_read_line", &[]).unwrap();
            Ok(Some((result, TurboTy::Str)))
        }
        "read_file" => compile_stdlib_1arg_rt(cx, args, "rt_read_file", TurboTy::Str),
        "write_file" => {
            if args.len() >= 2 {
                let (a, _) = compile_expr(cx, &args[0])?.unwrap();
                let (b, _) = compile_expr(cx, &args[1])?.unwrap();
                cx.rt_call("rt_write_file", &[a.into(), b.into()]);
            }
            Ok(None)
        }
        "pow" => {
            if args.len() >= 2 {
                let (a, _) = compile_expr(cx, &args[0])?.unwrap();
                let (b, _) = compile_expr(cx, &args[1])?.unwrap();
                let result = cx.rt_call("rt_pow", &[a.into(), b.into()]).unwrap();
                Ok(Some((result, TurboTy::Int)))
            } else {
                Ok(None)
            }
        }
        "sqrt" => {
            if !args.is_empty() {
                let (a, _) = compile_expr(cx, &args[0])?.unwrap();
                let result = cx.rt_call("rt_sqrt", &[a.into()]).unwrap();
                Ok(Some((result, TurboTy::Float)))
            } else {
                Ok(None)
            }
        }
        "sleep" => {
            if !args.is_empty() {
                let (a, _) = compile_expr(cx, &args[0])?.unwrap();
                cx.rt_call("rt_sleep_ms", &[a.into()]);
            }
            Ok(None)
        }
        "http_get" => compile_stdlib_1arg_rt(cx, args, "rt_http_get", TurboTy::Str),
        "http_post" => compile_stdlib_2arg_rt(cx, args, "rt_http_post", TurboTy::Str),
        "json_get" => compile_stdlib_2arg_rt(cx, args, "rt_json_get", TurboTy::Str),
        "channel" => {
            let result = cx.rt_call("rt_channel_create", &[]).unwrap();
            Ok(Some((result, TurboTy::Struct("Channel".to_string()))))
        }
        "send" => {
            if args.len() >= 2 {
                let (ch, _) = compile_expr(cx, &args[0])?.unwrap();
                let (val, _) = compile_expr(cx, &args[1])?.unwrap();
                let val_i64 = widen_for_storage(cx, val);
                let ch_ptr = int_to_ptr_if_needed(cx, ch);
                cx.rt_call("rt_channel_send", &[ch_ptr.into(), val_i64.into()]);
            }
            Ok(None)
        }
        "recv" => {
            if !args.is_empty() {
                let (ch, _) = compile_expr(cx, &args[0])?.unwrap();
                let ch_ptr = int_to_ptr_if_needed(cx, ch);
                let result = cx.rt_call("rt_channel_recv", &[ch_ptr.into()]).unwrap();
                Ok(Some((result, TurboTy::Int)))
            } else {
                Ok(None)
            }
        }
        "mutex" => {
            if !args.is_empty() {
                let (val, _) = compile_expr(cx, &args[0])?.unwrap();
                let val_i64 = widen_for_storage(cx, val);
                let result = cx.rt_call("rt_mutex_create", &[val_i64.into()]).unwrap();
                Ok(Some((result, TurboTy::Struct("Mutex".to_string()))))
            } else {
                Ok(None)
            }
        }
        "mutex_get" => {
            if !args.is_empty() {
                let (m, _) = compile_expr(cx, &args[0])?.unwrap();
                let m_ptr = int_to_ptr_if_needed(cx, m);
                let result = cx.rt_call("rt_mutex_get", &[m_ptr.into()]).unwrap();
                Ok(Some((result, TurboTy::Int)))
            } else {
                Ok(None)
            }
        }
        "mutex_set" => {
            if args.len() >= 2 {
                let (m, _) = compile_expr(cx, &args[0])?.unwrap();
                let (val, _) = compile_expr(cx, &args[1])?.unwrap();
                let m_ptr = int_to_ptr_if_needed(cx, m);
                let val_i64 = widen_for_storage(cx, val);
                cx.rt_call("rt_mutex_set", &[m_ptr.into(), val_i64.into()]);
            }
            Ok(None)
        }
        "hashmap" => {
            let result = cx.rt_call("rt_hashmap_new", &[]).unwrap();
            Ok(Some((result, TurboTy::Struct("HashMap".to_string()))))
        }
        "hashmap_set" => {
            if args.len() >= 3 {
                let (m, _) = compile_expr(cx, &args[0])?.unwrap();
                let (k, _) = compile_expr(cx, &args[1])?.unwrap();
                let (v, _) = compile_expr(cx, &args[2])?.unwrap();
                cx.rt_call("rt_hashmap_set", &[m.into(), k.into(), v.into()]);
            }
            Ok(None)
        }
        "hashmap_get" => compile_stdlib_2arg_rt(cx, args, "rt_hashmap_get", TurboTy::Str),
        "hashmap_has" => compile_stdlib_2arg_rt(cx, args, "rt_hashmap_has", TurboTy::Bool),
        "hashmap_len" => compile_stdlib_1arg_rt(cx, args, "rt_hashmap_len", TurboTy::Int),
        "hashmap_keys" => compile_stdlib_1arg_rt(
            cx,
            args,
            "rt_hashmap_keys",
            TurboTy::Array(Box::new(TurboTy::Str)),
        ),
        "hashmap_remove" => {
            if args.len() >= 2 {
                let (m, _) = compile_expr(cx, &args[0])?.unwrap();
                let (k, _) = compile_expr(cx, &args[1])?.unwrap();
                cx.rt_call("rt_hashmap_remove", &[m.into(), k.into()]);
            }
            Ok(None)
        }
        "map" => compile_builtin_map_llvm(cx, args),
        "filter" => compile_builtin_filter_llvm(cx, args),
        "reduce" => compile_builtin_reduce_llvm(cx, args),
        "clone" => {
            if !args.is_empty() {
                let (val, tty) = compile_expr(cx, &args[0])?.unwrap();
                // For structs with clone derive, call StructName__clone
                if let TurboTy::Struct(ref sname) = tty {
                    let clone_fn_name = format!("{sname}__clone");
                    if let Some(&clone_fn) = cx.user_fns.get(&clone_fn_name) {
                        let call = cx
                            .builder
                            .build_direct_call(clone_fn, &[val.into()], "")
                            .expect("build_direct_call");
                        return match call.try_as_basic_value().left() {
                            Some(v) => Ok(Some((v, tty))),
                            None => Ok(None),
                        };
                    }
                }
                // Otherwise, shallow clone (return same value for primitives)
                Ok(Some((val, tty)))
            } else {
                Ok(None)
            }
        }
        "deref" => {
            if !args.is_empty() {
                let (val, _) = compile_expr(cx, &args[0])?.unwrap();
                // deref: load i64 from pointer
                let i64_type = cx.context.i64_type();
                let ptr = cx
                    .builder
                    .build_int_to_ptr(
                        val.into_int_value(),
                        cx.context.ptr_type(AddressSpace::default()),
                        "deref_ptr",
                    )
                    .expect("itp");
                let loaded = cx
                    .builder
                    .build_load(i64_type, ptr, "deref_val")
                    .expect("load");
                Ok(Some((loaded, TurboTy::Int)))
            } else {
                Ok(None)
            }
        }
        "store" => {
            if args.len() >= 2 {
                let (ptr_val, _) = compile_expr(cx, &args[0])?.unwrap();
                let (val, _) = compile_expr(cx, &args[1])?.unwrap();
                let ptr = cx
                    .builder
                    .build_int_to_ptr(
                        ptr_val.into_int_value(),
                        cx.context.ptr_type(AddressSpace::default()),
                        "store_ptr",
                    )
                    .expect("itp");
                cx.builder.build_store(ptr, val).expect("store");
            }
            Ok(None)
        }
        "json_stringify" => {
            // rt_json_stringify(key_str, value_str) -> json_str
            if args.len() >= 2 {
                let (a, _) = compile_expr(cx, &args[0])?.unwrap();
                let (b, _) = compile_expr(cx, &args[1])?.unwrap();
                let result = cx
                    .rt_call("rt_json_stringify", &[a.into(), b.into()])
                    .unwrap();
                Ok(Some((result, TurboTy::Str)))
            } else if args.len() == 1 {
                let (a, _) = compile_expr(cx, &args[0])?.unwrap();
                let null_ptr = cx.context.ptr_type(AddressSpace::default()).const_null();
                let result = cx
                    .rt_call("rt_json_stringify", &[a.into(), null_ptr.into()])
                    .unwrap();
                Ok(Some((result, TurboTy::Str)))
            } else {
                Ok(None)
            }
        }
        "to_json" => {
            if !args.is_empty() {
                let (val, tty) = compile_expr(cx, &args[0])?.unwrap();
                if let TurboTy::Struct(ref sname) = tty {
                    compile_struct_to_json_llvm(cx, val, sname)
                } else {
                    let str_val = convert_to_str(cx, val, &tty)?;
                    Ok(Some((str_val, TurboTy::Str)))
                }
            } else {
                Ok(None)
            }
        }
        "to_json_array" => {
            if !args.is_empty() {
                let (arr_val, arr_tty) = compile_expr(cx, &args[0])?.unwrap();
                let elem_sname = match &arr_tty {
                    TurboTy::Array(inner) => match inner.as_ref() {
                        TurboTy::Struct(s) => Some(s.clone()),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(sname) = elem_sname {
                    compile_array_to_json_llvm(cx, arr_val, &sname)
                } else {
                    let str_val = convert_to_str(cx, arr_val, &arr_tty)?;
                    Ok(Some((str_val, TurboTy::Str)))
                }
            } else {
                Ok(None)
            }
        }
        "http_server" | "route" | "http_listen" | "respond" | "request_body" => {
            // Stub: these are complex HTTP server operations; emit a no-op for now
            for arg in args {
                compile_expr(cx, arg)?;
            }
            Ok(None)
        }
        _ => {
            // Check enum variant construction
            if !args.is_empty() {
                if let Expr::Ident(ref first_name) = args[0].node {
                    if let Some(variants) = cx.enum_variants.get(first_name.as_str()) {
                        if let Some(variant_index) = variants.iter().position(|v| v == name) {
                            let data_args = &args[1..];
                            let enum_name = first_name;

                            if let Some(&max_slots) = cx.enum_max_slots.get(enum_name.as_str()) {
                                let total_slots = 1 + max_slots;
                                let num_fields_val =
                                    cx.context.i64_type().const_int(total_slots as u64, false);
                                let ptr = cx
                                    .rt_call("rt_struct_alloc", &[num_fields_val.into()])
                                    .unwrap()
                                    .into_pointer_value();

                                // Store tag at offset 0
                                let tag_val =
                                    cx.context.i64_type().const_int(variant_index as u64, false);
                                cx.builder
                                    .build_store(ptr, tag_val)
                                    .expect("build_store failed");

                                // Store fields at offsets 8, 16, ...
                                for (j, arg) in data_args.iter().enumerate() {
                                    let (val, _) = compile_expr(cx, arg)?.unwrap();
                                    let offset = ((j + 1) * 8) as u64;
                                    let field_ptr = unsafe {
                                        cx.builder
                                            .build_gep(
                                                cx.context.i8_type(),
                                                ptr,
                                                &[cx.context.i64_type().const_int(offset, false)],
                                                "var_field_ptr",
                                            )
                                            .expect("build_gep failed")
                                    };
                                    let store_val = widen_for_storage(cx, val);
                                    cx.builder
                                        .build_store(field_ptr, store_val)
                                        .expect("build_store failed");
                                }

                                return Ok(Some((ptr.into(), TurboTy::Enum(enum_name.clone()))));
                            } else {
                                let val =
                                    cx.context.i64_type().const_int(variant_index as u64, false);
                                return Ok(Some((val.into(), TurboTy::Enum(enum_name.clone()))));
                            }
                        }
                    }
                }
            }

            // UFCS method call: parser rewrites obj.method(args) -> method(obj, args)
            if cx.user_fns.get(name.as_str()).is_none() && !args.is_empty() {
                let (first_val, first_tty) = compile_expr(cx, &args[0])?.unwrap();
                if let TurboTy::Struct(ref type_name) = first_tty {
                    let mangled = format!("{}__{}", type_name, name);
                    if let Some(&func) = cx.user_fns.get(&mangled) {
                        let mut arg_vals: Vec<BasicMetadataValueEnum> = vec![first_val.into()];
                        for arg in &args[1..] {
                            if let Some((v, _)) = compile_expr(cx, arg)? {
                                arg_vals.push(v.into());
                            }
                        }
                        let call = cx
                            .builder
                            .build_direct_call(func, &arg_vals, "")
                            .expect("build_direct_call failed");
                        let ret_tty = cx
                            .fn_ret_types
                            .get(&mangled)
                            .cloned()
                            .unwrap_or(TurboTy::Unit);
                        return match call.try_as_basic_value().left() {
                            Some(val) => Ok(Some((val, ret_tty))),
                            None => Ok(None),
                        };
                    }
                }
            }

            // Regular user function call
            let func = *cx.user_fns.get(name.as_str()).ok_or_else(|| CodegenError {
                code: ErrorCode::E0402,
                message: format!("undefined function: {name}"),
            })?;

            let ret_tty = cx
                .fn_ret_types
                .get(name.as_str())
                .cloned()
                .unwrap_or(TurboTy::Unit);

            let type_params = cx
                .fn_type_params
                .get(name.as_str())
                .cloned()
                .unwrap_or_default();

            let mut arg_vals: Vec<BasicMetadataValueEnum> = Vec::new();
            let mut arg_ttys: Vec<TurboTy> = Vec::new();
            for (i, arg) in args.iter().enumerate() {
                if let Some((val, tty)) = compile_expr(cx, arg)? {
                    // Type coercion: match parameter types
                    let expected_type = func.get_type().get_param_types();
                    if i < expected_type.len() {
                        let val = coerce_arg(cx, val, expected_type[i]);
                        arg_vals.push(val.into());
                    } else {
                        arg_vals.push(val.into());
                    }
                    arg_ttys.push(tty);
                }
            }

            // For generic functions, infer the actual return TurboTy from args.
            let actual_ret_tty = if !type_params.is_empty() {
                if let Some(f_def) = cx.fn_asts.get(name.as_str()) {
                    if let Some(ret_ty) = &f_def.return_type {
                        if let TypeExpr::Named(ref ret_name) = ret_ty.node {
                            if type_params.contains(ret_name) {
                                let mut inferred = None;
                                for (i, param) in f_def.params.iter().enumerate() {
                                    if let TypeExpr::Named(ref pname) = param.ty.node {
                                        if pname == ret_name {
                                            if i < arg_ttys.len() {
                                                inferred = Some(arg_ttys[i].clone());
                                            }
                                            break;
                                        }
                                    }
                                }
                                inferred.unwrap_or(ret_tty)
                            } else {
                                ret_tty
                            }
                        } else {
                            ret_tty
                        }
                    } else {
                        ret_tty
                    }
                } else {
                    ret_tty
                }
            } else {
                ret_tty
            };

            let call = cx
                .builder
                .build_direct_call(func, &arg_vals, "")
                .expect("build_direct_call failed");

            match call.try_as_basic_value().left() {
                Some(val) => Ok(Some((val, actual_ret_tty))),
                None => Ok(None),
            }
        }
    }
}

// ── Closure-based builtins (map, filter, reduce) ────────────────────

/// compile_builtin_map_llvm: map(arr, closure) -> [T]
fn compile_builtin_map_llvm<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.len() < 2 {
        return Ok(None);
    }
    let (arr_ptr, _arr_tty) = compile_expr(cx, &args[0])?.unwrap();
    let (closure_ptr_val, fn_tty) = compile_expr(cx, &args[1])?.unwrap();

    let (param_tty, ret_tty) = match &fn_tty {
        TurboTy::Fn(params, ret) => (params[0].clone(), *ret.clone()),
        _ => (TurboTy::Int, TurboTy::Int),
    };

    let ptr_type = cx.context.ptr_type(AddressSpace::default());
    let i8_type = cx.context.i8_type();
    let i64_type = cx.context.i64_type();

    let closure_ptr = closure_ptr_val.into_pointer_value();

    // Load fn_ptr (as i64) from offset 0
    let fn_ptr_i64 = cx
        .builder
        .build_load(i64_type, closure_ptr, "map_fn_i64")
        .expect("load")
        .into_int_value();
    let fn_ptr = cx
        .builder
        .build_int_to_ptr(fn_ptr_i64, ptr_type, "map_fn_ptr")
        .expect("itp");

    // Load env_ptr (as i64) from offset 8
    let env_slot = unsafe {
        cx.builder
            .build_gep(
                i8_type,
                closure_ptr,
                &[i64_type.const_int(8, false)],
                "env_slot",
            )
            .expect("gep")
    };
    let env_ptr_i64 = cx
        .builder
        .build_load(i64_type, env_slot, "map_env_i64")
        .expect("load")
        .into_int_value();
    let env_ptr = cx
        .builder
        .build_int_to_ptr(env_ptr_i64, ptr_type, "map_env_ptr")
        .expect("itp");

    // Get array length
    let arr_len = cx
        .rt_call("rt_array_len", &[arr_ptr.into()])
        .unwrap()
        .into_int_value();

    // Allocate result array
    let result_ptr = cx
        .rt_call("rt_array_alloc", &[arr_len.into()])
        .unwrap()
        .into_pointer_value();

    // Build function type for indirect call: (ptr, elem) -> ret
    let param_llvm = turbo_ty_to_llvm_ctx(&param_tty, cx.context, cx.enum_max_slots);
    let fn_type = if ret_tty == TurboTy::Unit {
        cx.context
            .void_type()
            .fn_type(&[ptr_type.into(), param_llvm.into()], false)
    } else {
        let ret_llvm = turbo_ty_to_llvm_ctx(&ret_tty, cx.context, cx.enum_max_slots);
        ret_llvm.fn_type(&[ptr_type.into(), param_llvm.into()], false)
    };

    // Loop: for i in 0..arr_len
    let current_fn = cx.current_fn;
    let header_block = cx.context.append_basic_block(current_fn, "map_header");
    let body_block = cx.context.append_basic_block(current_fn, "map_body");
    let exit_block = cx.context.append_basic_block(current_fn, "map_exit");

    // Allocate loop index on the stack
    let idx_alloca = cx
        .builder
        .build_alloca(i64_type, "map_idx")
        .expect("alloca");
    cx.builder
        .build_store(idx_alloca, i64_type.const_int(0, false))
        .expect("store");

    cx.builder
        .build_unconditional_branch(header_block)
        .expect("br");
    cx.builder.position_at_end(header_block);

    let idx = cx
        .builder
        .build_load(i64_type, idx_alloca, "idx")
        .expect("load")
        .into_int_value();
    let cond = cx
        .builder
        .build_int_compare(IntPredicate::SLT, idx, arr_len, "cond")
        .expect("cmp");
    cx.builder
        .build_conditional_branch(cond, body_block, exit_block)
        .expect("br");

    cx.builder.position_at_end(body_block);
    let idx2 = cx
        .builder
        .build_load(i64_type, idx_alloca, "idx2")
        .expect("load")
        .into_int_value();

    // Get element (as i64)
    let raw_elem = cx
        .rt_call("rt_array_get", &[arr_ptr.into(), idx2.into()])
        .unwrap()
        .into_int_value();
    // Narrow to param type
    let typed_elem: BasicValueEnum = match &param_tty {
        TurboTy::Bool => cx
            .builder
            .build_int_truncate(raw_elem, cx.context.bool_type(), "trunc")
            .expect("trunc")
            .into(),
        TurboTy::Float => cx
            .builder
            .build_bit_cast(raw_elem, cx.context.f64_type(), "f2i")
            .expect("bc")
            .into(),
        _ => raw_elem.into(),
    };

    // Call closure
    let mapped_val = cx
        .builder
        .build_indirect_call(
            fn_type,
            fn_ptr,
            &[env_ptr.into(), typed_elem.into()],
            "mapped",
        )
        .expect("indirect_call");

    // Store result
    if let Some(mapped_basic) = mapped_val.try_as_basic_value().left() {
        let store_val = widen_for_storage(cx, mapped_basic);
        let idx3 = cx
            .builder
            .build_load(i64_type, idx_alloca, "idx3")
            .expect("load")
            .into_int_value();
        cx.rt_call(
            "rt_array_set",
            &[result_ptr.into(), idx3.into(), store_val.into()],
        );
    }

    let idx4 = cx
        .builder
        .build_load(i64_type, idx_alloca, "idx4")
        .expect("load")
        .into_int_value();
    let one = i64_type.const_int(1, false);
    let next_idx = cx
        .builder
        .build_int_add(idx4, one, "next_idx")
        .expect("add");
    cx.builder.build_store(idx_alloca, next_idx).expect("store");
    cx.builder
        .build_unconditional_branch(header_block)
        .expect("br");

    cx.builder.position_at_end(exit_block);

    Ok(Some((result_ptr.into(), TurboTy::Array(Box::new(ret_tty)))))
}

/// compile_builtin_filter_llvm: filter(arr, closure) -> [T]
fn compile_builtin_filter_llvm<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.len() < 2 {
        return Ok(None);
    }
    let (arr_ptr, arr_tty) = compile_expr(cx, &args[0])?.unwrap();
    let (closure_ptr_val, fn_tty) = compile_expr(cx, &args[1])?.unwrap();

    let elem_tty = match &arr_tty {
        TurboTy::Array(inner) => *inner.clone(),
        _ => TurboTy::Int,
    };
    let param_tty = match &fn_tty {
        TurboTy::Fn(params, _) => params[0].clone(),
        _ => TurboTy::Int,
    };

    let ptr_type = cx.context.ptr_type(AddressSpace::default());
    let i8_type = cx.context.i8_type();
    let i64_type = cx.context.i64_type();

    let closure_ptr = closure_ptr_val.into_pointer_value();
    let fn_ptr_i64 = cx
        .builder
        .build_load(i64_type, closure_ptr, "filt_fn_i64")
        .expect("load")
        .into_int_value();
    let fn_ptr = cx
        .builder
        .build_int_to_ptr(fn_ptr_i64, ptr_type, "filt_fn_ptr")
        .expect("itp");
    let env_slot = unsafe {
        cx.builder
            .build_gep(
                i8_type,
                closure_ptr,
                &[i64_type.const_int(8, false)],
                "env_slot",
            )
            .expect("gep")
    };
    let env_ptr_i64 = cx
        .builder
        .build_load(i64_type, env_slot, "filt_env_i64")
        .expect("load")
        .into_int_value();
    let env_ptr = cx
        .builder
        .build_int_to_ptr(env_ptr_i64, ptr_type, "filt_env_ptr")
        .expect("itp");

    let arr_len = cx
        .rt_call("rt_array_len", &[arr_ptr.into()])
        .unwrap()
        .into_int_value();
    let result_ptr = cx
        .rt_call("rt_array_alloc", &[arr_len.into()])
        .unwrap()
        .into_pointer_value();

    let param_llvm = turbo_ty_to_llvm_ctx(&param_tty, cx.context, cx.enum_max_slots);
    let bool_type = cx.context.bool_type();
    let fn_type = bool_type.fn_type(&[ptr_type.into(), param_llvm.into()], false);

    let current_fn = cx.current_fn;
    let header_block = cx.context.append_basic_block(current_fn, "filt_header");
    let body_block = cx.context.append_basic_block(current_fn, "filt_body");
    let store_block = cx.context.append_basic_block(current_fn, "filt_store");
    let inc_block = cx.context.append_basic_block(current_fn, "filt_inc");
    let exit_block = cx.context.append_basic_block(current_fn, "filt_exit");

    let idx_alloca = cx
        .builder
        .build_alloca(i64_type, "filt_idx")
        .expect("alloca");
    let out_idx_alloca = cx
        .builder
        .build_alloca(i64_type, "filt_out_idx")
        .expect("alloca");
    cx.builder
        .build_store(idx_alloca, i64_type.const_int(0, false))
        .expect("store");
    cx.builder
        .build_store(out_idx_alloca, i64_type.const_int(0, false))
        .expect("store");

    cx.builder
        .build_unconditional_branch(header_block)
        .expect("br");
    cx.builder.position_at_end(header_block);
    let idx = cx
        .builder
        .build_load(i64_type, idx_alloca, "idx")
        .expect("load")
        .into_int_value();
    let cond = cx
        .builder
        .build_int_compare(IntPredicate::SLT, idx, arr_len, "cond")
        .expect("cmp");
    cx.builder
        .build_conditional_branch(cond, body_block, exit_block)
        .expect("br");

    cx.builder.position_at_end(body_block);
    let idx2 = cx
        .builder
        .build_load(i64_type, idx_alloca, "idx2")
        .expect("load")
        .into_int_value();
    let raw_elem = cx
        .rt_call("rt_array_get", &[arr_ptr.into(), idx2.into()])
        .unwrap()
        .into_int_value();
    let typed_elem: BasicValueEnum = match &param_tty {
        TurboTy::Bool => cx
            .builder
            .build_int_truncate(raw_elem, bool_type, "trunc")
            .expect("trunc")
            .into(),
        TurboTy::Float => cx
            .builder
            .build_bit_cast(raw_elem, cx.context.f64_type(), "bc")
            .expect("bc")
            .into(),
        _ => raw_elem.into(),
    };
    let pred_val = cx
        .builder
        .build_indirect_call(
            fn_type,
            fn_ptr,
            &[env_ptr.into(), typed_elem.into()],
            "pred",
        )
        .expect("indirect_call");
    let keep = match pred_val.try_as_basic_value().left() {
        Some(BasicValueEnum::IntValue(v)) => v,
        _ => i64_type.const_int(0, false),
    };
    let zero8 = cx.context.bool_type().const_int(0, false);
    let keep_bool = cx
        .builder
        .build_int_compare(IntPredicate::NE, keep, zero8, "keep")
        .expect("cmp");
    cx.builder
        .build_conditional_branch(keep_bool, store_block, inc_block)
        .expect("br");

    cx.builder.position_at_end(store_block);
    let raw_elem2 = cx
        .rt_call("rt_array_get", &[arr_ptr.into(), idx2.into()])
        .unwrap()
        .into_int_value();
    let out_idx = cx
        .builder
        .build_load(i64_type, out_idx_alloca, "out_idx")
        .expect("load")
        .into_int_value();
    cx.rt_call(
        "rt_array_set",
        &[result_ptr.into(), out_idx.into(), raw_elem2.into()],
    );
    let one = i64_type.const_int(1, false);
    let next_out = cx
        .builder
        .build_int_add(out_idx, one, "next_out")
        .expect("add");
    cx.builder
        .build_store(out_idx_alloca, next_out)
        .expect("store");
    cx.builder
        .build_unconditional_branch(inc_block)
        .expect("br");

    cx.builder.position_at_end(inc_block);
    let idx3 = cx
        .builder
        .build_load(i64_type, idx_alloca, "idx3")
        .expect("load")
        .into_int_value();
    let one2 = i64_type.const_int(1, false);
    let next_idx = cx
        .builder
        .build_int_add(idx3, one2, "next_idx")
        .expect("add");
    cx.builder.build_store(idx_alloca, next_idx).expect("store");
    cx.builder
        .build_unconditional_branch(header_block)
        .expect("br");

    cx.builder.position_at_end(exit_block);
    // Update result array length to actual number of kept elements
    let final_out_idx = cx
        .builder
        .build_load(i64_type, out_idx_alloca, "final_out")
        .expect("load");
    cx.builder
        .build_store(result_ptr, final_out_idx)
        .expect("store");

    Ok(Some((result_ptr.into(), arr_tty)))
}

/// compile_builtin_reduce_llvm: reduce(arr, init, closure) -> T
fn compile_builtin_reduce_llvm<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.len() < 3 {
        return Ok(None);
    }
    let (arr_ptr, arr_tty) = compile_expr(cx, &args[0])?.unwrap();
    let (init_val, init_tty) = compile_expr(cx, &args[1])?.unwrap();
    let (closure_ptr_val, _fn_tty) = compile_expr(cx, &args[2])?.unwrap();

    let elem_tty = match &arr_tty {
        TurboTy::Array(inner) => *inner.clone(),
        _ => TurboTy::Int,
    };

    let ptr_type = cx.context.ptr_type(AddressSpace::default());
    let i8_type = cx.context.i8_type();
    let i64_type = cx.context.i64_type();

    let closure_ptr = closure_ptr_val.into_pointer_value();
    let fn_ptr_i64 = cx
        .builder
        .build_load(i64_type, closure_ptr, "red_fn_i64")
        .expect("load")
        .into_int_value();
    let fn_ptr = cx
        .builder
        .build_int_to_ptr(fn_ptr_i64, ptr_type, "red_fn_ptr")
        .expect("itp");
    let env_slot = unsafe {
        cx.builder
            .build_gep(
                i8_type,
                closure_ptr,
                &[i64_type.const_int(8, false)],
                "env_slot",
            )
            .expect("gep")
    };
    let env_ptr_i64 = cx
        .builder
        .build_load(i64_type, env_slot, "red_env_i64")
        .expect("load")
        .into_int_value();
    let env_ptr = cx
        .builder
        .build_int_to_ptr(env_ptr_i64, ptr_type, "red_env_ptr")
        .expect("itp");

    let arr_len = cx
        .rt_call("rt_array_len", &[arr_ptr.into()])
        .unwrap()
        .into_int_value();

    // Accumulator stored on stack (as i64)
    let acc_alloca = cx.builder.build_alloca(i64_type, "acc").expect("alloca");
    let init_i64 = widen_for_storage(cx, init_val);
    cx.builder.build_store(acc_alloca, init_i64).expect("store");

    let elem_llvm = turbo_ty_to_llvm_ctx(&elem_tty, cx.context, cx.enum_max_slots);
    let acc_llvm = turbo_ty_to_llvm_ctx(&init_tty, cx.context, cx.enum_max_slots);
    // closure: (env, acc, elem) -> acc
    let fn_type = acc_llvm.fn_type(&[ptr_type.into(), acc_llvm.into(), elem_llvm.into()], false);

    let current_fn = cx.current_fn;
    let header_block = cx.context.append_basic_block(current_fn, "red_header");
    let body_block = cx.context.append_basic_block(current_fn, "red_body");
    let exit_block = cx.context.append_basic_block(current_fn, "red_exit");

    let idx_alloca = cx
        .builder
        .build_alloca(i64_type, "red_idx")
        .expect("alloca");
    cx.builder
        .build_store(idx_alloca, i64_type.const_int(0, false))
        .expect("store");

    cx.builder
        .build_unconditional_branch(header_block)
        .expect("br");
    cx.builder.position_at_end(header_block);
    let idx = cx
        .builder
        .build_load(i64_type, idx_alloca, "idx")
        .expect("load")
        .into_int_value();
    let cond = cx
        .builder
        .build_int_compare(IntPredicate::SLT, idx, arr_len, "cond")
        .expect("cmp");
    cx.builder
        .build_conditional_branch(cond, body_block, exit_block)
        .expect("br");

    cx.builder.position_at_end(body_block);
    let idx2 = cx
        .builder
        .build_load(i64_type, idx_alloca, "idx2")
        .expect("load")
        .into_int_value();
    let raw_elem = cx
        .rt_call("rt_array_get", &[arr_ptr.into(), idx2.into()])
        .unwrap()
        .into_int_value();
    let acc_i64 = cx
        .builder
        .build_load(i64_type, acc_alloca, "acc_i64")
        .expect("load")
        .into_int_value();

    // Narrow both for the call
    let typed_elem: BasicValueEnum = match &elem_tty {
        TurboTy::Float => cx
            .builder
            .build_bit_cast(raw_elem, cx.context.f64_type(), "bc")
            .expect("bc")
            .into(),
        _ => raw_elem.into(),
    };
    let typed_acc: BasicValueEnum = match &init_tty {
        TurboTy::Float => cx
            .builder
            .build_bit_cast(acc_i64, cx.context.f64_type(), "bc_acc")
            .expect("bc")
            .into(),
        _ => acc_i64.into(),
    };

    let new_acc = cx
        .builder
        .build_indirect_call(
            fn_type,
            fn_ptr,
            &[env_ptr.into(), typed_acc.into(), typed_elem.into()],
            "new_acc",
        )
        .expect("indirect_call");

    if let Some(new_acc_val) = new_acc.try_as_basic_value().left() {
        let new_acc_i64 = widen_for_storage(cx, new_acc_val);
        cx.builder
            .build_store(acc_alloca, new_acc_i64)
            .expect("store");
    }

    let one = i64_type.const_int(1, false);
    let idx3 = cx
        .builder
        .build_load(i64_type, idx_alloca, "idx3")
        .expect("load")
        .into_int_value();
    let next_idx = cx
        .builder
        .build_int_add(idx3, one, "next_idx")
        .expect("add");
    cx.builder.build_store(idx_alloca, next_idx).expect("store");
    cx.builder
        .build_unconditional_branch(header_block)
        .expect("br");

    cx.builder.position_at_end(exit_block);
    let final_acc_i64 = cx
        .builder
        .build_load(i64_type, acc_alloca, "final_acc")
        .expect("load")
        .into_int_value();
    let final_acc: BasicValueEnum = match &init_tty {
        TurboTy::Float => cx
            .builder
            .build_bit_cast(final_acc_i64, cx.context.f64_type(), "bc_final")
            .expect("bc")
            .into(),
        _ => final_acc_i64.into(),
    };

    Ok(Some((final_acc, init_tty)))
}

// ── Built-in functions ──────────────────────────────────────────────

/// Serialize a struct to JSON: {"field1":val1,"field2":val2}
fn compile_struct_to_json_llvm<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    struct_ptr: BasicValueEnum<'ctx>,
    struct_name: &str,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    let struct_layout = cx
        .struct_fields
        .get(struct_name)
        .ok_or_else(|| CodegenError {
            code: ErrorCode::E0400,
            message: format!("undefined struct: {struct_name}"),
        })?
        .clone();

    let i8_type = cx.context.i8_type();
    let i64_type = cx.context.i64_type();
    let ptr = struct_ptr.into_pointer_value();

    let mut result: BasicValueEnum = cx.create_string("{")?.into();

    for (i, (field_name, field_ty)) in struct_layout.iter().enumerate() {
        // Add key prefix
        let prefix = if i > 0 {
            format!(",\"{}\":", field_name)
        } else {
            format!("\"{}\":", field_name)
        };
        let prefix_ptr = cx.create_string(&prefix)?;
        result = cx
            .rt_call("rt_str_concat", &[result.into(), prefix_ptr.into()])
            .unwrap();

        // Load field value
        let offset = (i * 8) as u64;
        let field_ptr = if offset == 0 {
            ptr
        } else {
            unsafe {
                cx.builder
                    .build_gep(
                        i8_type,
                        ptr,
                        &[i64_type.const_int(offset, false)],
                        "json_field_ptr",
                    )
                    .expect("gep")
            }
        };
        let raw_val = cx
            .builder
            .build_load(i64_type, field_ptr, "json_field_val")
            .expect("load");

        // Convert field to JSON string representation
        let field_str = match field_ty {
            TurboTy::Str => {
                let str_ptr = cx
                    .builder
                    .build_int_to_ptr(
                        raw_val.into_int_value(),
                        cx.context.ptr_type(AddressSpace::default()),
                        "str_ptr",
                    )
                    .expect("itp");
                let quote = cx.create_string("\"")?;
                let tmp = cx
                    .rt_call("rt_str_concat", &[quote.into(), str_ptr.into()])
                    .unwrap();
                let quote2 = cx.create_string("\"")?;
                cx.rt_call("rt_str_concat", &[tmp.into(), quote2.into()])
                    .unwrap()
            }
            TurboTy::Int => cx.rt_call("rt_i64_to_str", &[raw_val.into()]).unwrap(),
            TurboTy::Bool => {
                let bool_val = cx
                    .builder
                    .build_int_truncate(raw_val.into_int_value(), cx.context.i8_type(), "trunc")
                    .expect("trunc");
                cx.rt_call("rt_bool_to_str", &[bool_val.into()]).unwrap()
            }
            TurboTy::Float => {
                let fval = cx
                    .builder
                    .build_bit_cast(raw_val.into_int_value(), cx.context.f64_type(), "i2f")
                    .expect("bc");
                cx.rt_call("rt_f64_to_str", &[fval.into()]).unwrap()
            }
            _ => cx.rt_call("rt_i64_to_str", &[raw_val.into()]).unwrap(),
        };

        result = cx
            .rt_call("rt_str_concat", &[result.into(), field_str.into()])
            .unwrap();
    }

    let suffix = cx.create_string("}")?;
    result = cx
        .rt_call("rt_str_concat", &[result.into(), suffix.into()])
        .unwrap();

    Ok(Some((result, TurboTy::Str)))
}

/// Serialize an array of structs to JSON: [item1,item2,...]
fn compile_array_to_json_llvm<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    arr_val: BasicValueEnum<'ctx>,
    struct_name: &str,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    let i64_type = cx.context.i64_type();
    let arr_len = cx
        .rt_call("rt_array_len", &[arr_val.into()])
        .unwrap()
        .into_int_value();

    let mut result: BasicValueEnum = cx.create_string("[")?.into();

    let current_fn = cx.current_fn;
    let header = cx.context.append_basic_block(current_fn, "json_arr_header");
    let body = cx.context.append_basic_block(current_fn, "json_arr_body");
    let exit = cx.context.append_basic_block(current_fn, "json_arr_exit");

    let idx_alloca = cx
        .builder
        .build_alloca(i64_type, "json_idx")
        .expect("alloca");
    let result_alloca = cx
        .builder
        .build_alloca(cx.context.ptr_type(AddressSpace::default()), "json_result")
        .expect("alloca");
    cx.builder
        .build_store(idx_alloca, i64_type.const_int(0, false))
        .expect("store");
    cx.builder
        .build_store(result_alloca, result.into_pointer_value())
        .expect("store");
    cx.builder.build_unconditional_branch(header).expect("br");

    cx.builder.position_at_end(header);
    let idx = cx
        .builder
        .build_load(i64_type, idx_alloca, "idx")
        .expect("load")
        .into_int_value();
    let cond = cx
        .builder
        .build_int_compare(IntPredicate::SLT, idx, arr_len, "cond")
        .expect("cmp");
    cx.builder
        .build_conditional_branch(cond, body, exit)
        .expect("br");

    cx.builder.position_at_end(body);
    let idx2 = cx
        .builder
        .build_load(i64_type, idx_alloca, "idx2")
        .expect("load")
        .into_int_value();
    let cur_result = cx
        .builder
        .build_load(
            cx.context.ptr_type(AddressSpace::default()),
            result_alloca,
            "cur",
        )
        .expect("load");

    // Add comma if not first
    let zero = i64_type.const_int(0, false);
    let is_first = cx
        .builder
        .build_int_compare(IntPredicate::EQ, idx2, zero, "is_first")
        .expect("cmp");
    let comma_ptr = cx.create_string(",")?;
    let empty_ptr = cx.create_string("")?;
    let sep = cx
        .builder
        .build_select(is_first, empty_ptr, comma_ptr, "sep")
        .expect("select");
    let with_sep = cx
        .rt_call("rt_str_concat", &[cur_result.into(), sep.into()])
        .unwrap();

    // Get element and serialize
    let elem = cx
        .rt_call("rt_array_get", &[arr_val.into(), idx2.into()])
        .unwrap();
    let elem_ptr = cx
        .builder
        .build_int_to_ptr(
            elem.into_int_value(),
            cx.context.ptr_type(AddressSpace::default()),
            "elem_ptr",
        )
        .expect("itp");
    let sname = struct_name.to_string();
    let (elem_json, _) = compile_struct_to_json_llvm(cx, elem_ptr.into(), &sname)?.unwrap();
    let new_result = cx
        .rt_call("rt_str_concat", &[with_sep.into(), elem_json.into()])
        .unwrap();
    cx.builder
        .build_store(result_alloca, new_result.into_pointer_value())
        .expect("store");

    let one = i64_type.const_int(1, false);
    let next = cx.builder.build_int_add(idx2, one, "next").expect("add");
    cx.builder.build_store(idx_alloca, next).expect("store");
    cx.builder.build_unconditional_branch(header).expect("br");

    cx.builder.position_at_end(exit);
    let final_result = cx
        .builder
        .build_load(
            cx.context.ptr_type(AddressSpace::default()),
            result_alloca,
            "final",
        )
        .expect("load");
    let suffix = cx.create_string("]")?;
    let done = cx
        .rt_call("rt_str_concat", &[final_result.into(), suffix.into()])
        .unwrap();

    Ok(Some((done, TurboTy::Str)))
}

fn compile_print<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.is_empty() {
        let ptr = cx.create_string("")?;
        cx.rt_call("rt_print_str", &[ptr.into()]);
        return Ok(None);
    }

    let result = compile_expr(cx, &args[0])?;
    if let Some((v, tty)) = result {
        match tty {
            TurboTy::Str => {
                // Generic functions return i64 even for Str; convert to ptr if needed
                let ptr_val: BasicValueEnum = match v {
                    BasicValueEnum::PointerValue(_) => v,
                    BasicValueEnum::IntValue(iv) => cx
                        .builder
                        .build_int_to_ptr(
                            iv,
                            cx.context.ptr_type(AddressSpace::default()),
                            "str_ptr",
                        )
                        .expect("itp")
                        .into(),
                    _ => v,
                };
                cx.rt_call("rt_print_str", &[ptr_val.into()]);
            }
            TurboTy::Float => {
                cx.rt_call("rt_print_f64", &[v.into()]);
            }
            TurboTy::Bool => {
                let iv = v.into_int_value();
                let iv = if iv.get_type().get_bit_width() > 8 {
                    cx.builder
                        .build_int_truncate(iv, cx.context.i8_type(), "tobool")
                        .expect("build_int_truncate failed")
                } else {
                    iv
                };
                cx.rt_call("rt_print_bool", &[iv.into()]);
            }
            TurboTy::Int => {
                let iv = v.into_int_value();
                let iv = if iv.get_type().get_bit_width() < 64 {
                    cx.builder
                        .build_int_s_extend(iv, cx.context.i64_type(), "ext")
                        .expect("build_int_s_extend failed")
                } else {
                    iv
                };
                cx.rt_call("rt_print_i64", &[iv.into()]);
            }
            TurboTy::Unit => {
                let ptr = cx.create_string("()")?;
                cx.rt_call("rt_print_str", &[ptr.into()]);
            }
            TurboTy::Struct(ref sname) => {
                // If Display is derived, call StructName__to_string
                let sname = sname.clone();
                let to_str_fn = format!("{sname}__to_string");
                if let Some(&ts_fn) = cx.user_fns.get(&to_str_fn) {
                    let s = cx
                        .builder
                        .build_direct_call(ts_fn, &[v.into()], "to_str")
                        .expect("call")
                        .try_as_basic_value()
                        .left()
                        .unwrap();
                    cx.rt_call("rt_print_str", &[s.into()]);
                } else {
                    // No Display, print struct name as placeholder
                    let ptr = cx.create_string(&format!("<{sname}>"))?;
                    cx.rt_call("rt_print_str", &[ptr.into()]);
                }
            }
            TurboTy::Array(_) => {
                // Print array as a bracketed list via rt_array_print_str if available
                let ptr = cx.create_string("<array>")?;
                cx.rt_call("rt_print_str", &[ptr.into()]);
            }
            TurboTy::Enum(_) => {
                // Enums: print the integer tag value
                let iv = match v {
                    BasicValueEnum::IntValue(i) => {
                        if i.get_type().get_bit_width() < 64 {
                            cx.builder
                                .build_int_s_extend(i, cx.context.i64_type(), "ext")
                                .expect("extend")
                        } else {
                            i
                        }
                    }
                    BasicValueEnum::PointerValue(p) => cx
                        .builder
                        .build_ptr_to_int(p, cx.context.i64_type(), "p2i")
                        .expect("p2i"),
                    _ => cx.context.i64_type().const_int(0, false),
                };
                cx.rt_call("rt_print_i64", &[iv.into()]);
            }
            TurboTy::Result(_, _) | TurboTy::Optional(_) => {
                let ptr = cx.rt_call("rt_result_to_str", &[v.into()]);
                if let Some(s) = ptr {
                    cx.rt_call("rt_print_str", &[s.into()]);
                } else {
                    let ptr = cx.create_string("<result>")?;
                    cx.rt_call("rt_print_str", &[ptr.into()]);
                }
            }
            _ => {
                // For other types, print as integer (best-effort)
                let iv = match v {
                    BasicValueEnum::IntValue(i) => i,
                    BasicValueEnum::PointerValue(p) => cx
                        .builder
                        .build_ptr_to_int(p, cx.context.i64_type(), "p2i")
                        .expect("p2i"),
                    _ => cx.context.i64_type().const_int(0, false),
                };
                cx.rt_call("rt_print_i64", &[iv.into()]);
            }
        }
    }
    Ok(None)
}

fn compile_panic<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.is_empty() {
        let ptr = cx.create_string("explicit panic")?;
        cx.rt_call("rt_panic", &[ptr.into()]);
    } else {
        let (val, _) = compile_expr(cx, &args[0])?.unwrap();
        cx.rt_call("rt_panic", &[val.into()]);
    }
    cx.builder
        .build_unreachable()
        .expect("build_unreachable failed");
    let dead = cx.context.append_basic_block(cx.current_fn, "after_panic");
    cx.builder.position_at_end(dead);
    Ok(None)
}

fn compile_assert<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.is_empty() {
        return Ok(None);
    }
    let (cond, _) = compile_expr(cx, &args[0])?.unwrap();
    let cond_bool = cx.to_bool(cond);

    let fail_block = cx.context.append_basic_block(cx.current_fn, "assert_fail");
    let ok_block = cx.context.append_basic_block(cx.current_fn, "assert_ok");

    cx.builder
        .build_conditional_branch(cond_bool, ok_block, fail_block)
        .expect("build_conditional_branch failed");

    cx.builder.position_at_end(fail_block);
    if args.len() > 1 {
        let (msg, _) = compile_expr(cx, &args[1])?.unwrap();
        cx.rt_call("rt_assert_fail", &[msg.into()]);
    } else {
        let ptr = cx.create_string("assertion failed")?;
        cx.rt_call("rt_assert_fail", &[ptr.into()]);
    }
    cx.builder
        .build_unreachable()
        .expect("build_unreachable failed");

    cx.builder.position_at_end(ok_block);
    Ok(None)
}

fn compile_assert_eq<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
    negate: bool,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.len() < 2 {
        return Ok(None);
    }
    let (a, a_tty) = compile_expr(cx, &args[0])?.unwrap();
    let (b, _) = compile_expr(cx, &args[1])?.unwrap();

    let eq = if matches!(a_tty, TurboTy::Str) || a.is_pointer_value() && b.is_pointer_value() {
        // String comparison via rt_str_eq
        let eq_val = cx
            .rt_call("rt_str_eq", &[a.into(), b.into()])
            .unwrap()
            .into_int_value();
        let zero = cx.context.i8_type().const_int(0, false);
        let cmp = cx
            .builder
            .build_int_compare(IntPredicate::NE, eq_val, zero, "str_eq")
            .expect("icmp");
        if negate {
            cx.builder.build_not(cmp, "not_eq").expect("not")
        } else {
            cmp
        }
    } else {
        let ai = a.into_int_value();
        let bi = b.into_int_value();
        let pred = if negate {
            IntPredicate::NE
        } else {
            IntPredicate::EQ
        };
        cx.builder
            .build_int_compare(pred, ai, bi, "assert_eq")
            .expect("build_int_compare failed")
    };

    let fail_block = cx
        .context
        .append_basic_block(cx.current_fn, "assert_eq_fail");
    let ok_block = cx.context.append_basic_block(cx.current_fn, "assert_eq_ok");

    cx.builder
        .build_conditional_branch(eq, ok_block, fail_block)
        .expect("build_conditional_branch failed");

    cx.builder.position_at_end(fail_block);
    // Simple assert fail for now
    let ptr = cx.create_string("assertion failed: values not equal")?;
    cx.rt_call("rt_assert_fail", &[ptr.into()]);
    cx.builder
        .build_unreachable()
        .expect("build_unreachable failed");

    cx.builder.position_at_end(ok_block);
    Ok(None)
}

fn compile_len<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.is_empty() {
        return Ok(None);
    }
    let (val, tty) = compile_expr(cx, &args[0])?.unwrap();
    let result = match tty {
        TurboTy::Str => cx.rt_call("rt_str_len", &[val.into()]).unwrap(),
        TurboTy::Array(_) => cx.rt_call("rt_array_len", &[val.into()]).unwrap(),
        _ => cx.context.i64_type().const_int(0, false).into(),
    };
    Ok(Some((result, TurboTy::Int)))
}

fn compile_abs<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.is_empty() {
        return Ok(None);
    }
    let (val, tty) = compile_expr(cx, &args[0])?.unwrap();
    match tty {
        TurboTy::Int => {
            let iv = val.into_int_value();
            let zero = cx.context.i64_type().const_int(0, false);
            let neg = cx
                .builder
                .build_int_neg(iv, "neg")
                .expect("build_int_neg failed");
            let is_neg = cx
                .builder
                .build_int_compare(IntPredicate::SLT, iv, zero, "is_neg")
                .expect("build_int_compare failed");
            let result: BasicValueEnum = cx
                .builder
                .build_select(
                    is_neg,
                    BasicValueEnum::IntValue(neg),
                    BasicValueEnum::IntValue(iv),
                    "abs",
                )
                .expect("build_select failed");
            Ok(Some((result, TurboTy::Int)))
        }
        TurboTy::Float => {
            // Use llvm.fabs intrinsic via negation + select
            let fv = val.into_float_value();
            let zero = cx.context.f64_type().const_float(0.0);
            let neg = cx
                .builder
                .build_float_neg(fv, "fneg")
                .expect("build_float_neg failed");
            let is_neg = cx
                .builder
                .build_float_compare(FloatPredicate::OLT, fv, zero, "is_neg")
                .expect("build_float_compare failed");
            let result: BasicValueEnum = cx
                .builder
                .build_select(
                    is_neg,
                    BasicValueEnum::FloatValue(neg),
                    BasicValueEnum::FloatValue(fv),
                    "fabs",
                )
                .expect("build_select failed");
            Ok(Some((result, TurboTy::Float)))
        }
        _ => Ok(Some((val, tty))),
    }
}

fn compile_min_max<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
    is_min: bool,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.len() < 2 {
        return Ok(None);
    }
    let (a, tty) = compile_expr(cx, &args[0])?.unwrap();
    let (b, _) = compile_expr(cx, &args[1])?.unwrap();

    let cmp_pred = if is_min {
        IntPredicate::SLT
    } else {
        IntPredicate::SGT
    };

    let ai = a.into_int_value();
    let bi = b.into_int_value();
    let cond = cx
        .builder
        .build_int_compare(cmp_pred, ai, bi, "cmp")
        .expect("build_int_compare failed");
    let result: BasicValueEnum = cx
        .builder
        .build_select(
            cond,
            BasicValueEnum::IntValue(ai),
            BasicValueEnum::IntValue(bi),
            if is_min { "min" } else { "max" },
        )
        .expect("build_select failed");
    Ok(Some((result, tty)))
}

fn compile_to_str_builtin<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.is_empty() {
        return Ok(None);
    }
    let (val, tty) = compile_expr(cx, &args[0])?.unwrap();
    let str_val = convert_to_str(cx, val, &tty)?;
    Ok(Some((str_val, TurboTy::Str)))
}

fn compile_stdlib_1arg_rt<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
    rt_name: &str,
    ret_tty: TurboTy,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.is_empty() {
        return Ok(None);
    }
    let (a, _) = compile_expr(cx, &args[0])?.unwrap();
    let result = cx.rt_call(rt_name, &[a.into()]).unwrap();
    Ok(Some((result, ret_tty)))
}

fn compile_stdlib_2arg_rt<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    args: &[Spanned<Expr>],
    rt_name: &str,
    ret_tty: TurboTy,
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    if args.len() < 2 {
        return Ok(None);
    }
    let (a, _) = compile_expr(cx, &args[0])?.unwrap();
    let (b, _) = compile_expr(cx, &args[1])?.unwrap();
    let result = cx.rt_call(rt_name, &[a.into(), b.into()]).unwrap();
    Ok(Some((result, ret_tty)))
}

#[allow(unused_variables)]
fn compile_method_call<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    obj: BasicValueEnum<'ctx>,
    obj_tty: &TurboTy,
    field: &str,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped<'ctx>, CodegenError> {
    Err(CodegenError {
        code: ErrorCode::E0400,
        message: format!("LLVM: method call `{field}` not yet implemented"),
    })
}

// ── Value conversion helpers ────────────────────────────────────────

fn convert_to_str<'a, 'ctx>(
    cx: &mut Ctx<'a, 'ctx>,
    val: BasicValueEnum<'ctx>,
    tty: &TurboTy,
) -> Result<BasicValueEnum<'ctx>, CodegenError> {
    match tty {
        TurboTy::Str => Ok(val),
        TurboTy::Int => {
            let iv = val.into_int_value();
            let iv = if iv.get_type().get_bit_width() < 64 {
                cx.builder
                    .build_int_s_extend(iv, cx.context.i64_type(), "ext")
                    .expect("build_int_s_extend failed")
            } else {
                iv
            };
            Ok(cx.rt_call("rt_i64_to_str", &[iv.into()]).unwrap())
        }
        TurboTy::Float => Ok(cx.rt_call("rt_f64_to_str", &[val.into()]).unwrap()),
        TurboTy::Bool => {
            let iv = val.into_int_value();
            let iv = if iv.get_type().get_bit_width() > 8 {
                cx.builder
                    .build_int_truncate(iv, cx.context.i8_type(), "trunc")
                    .expect("build_int_truncate failed")
            } else {
                iv
            };
            Ok(cx.rt_call("rt_bool_to_str", &[iv.into()]).unwrap())
        }
        TurboTy::Struct(ref sname) => {
            let sname = sname.clone();
            let to_str_fn = format!("{sname}__to_string");
            if let Some(&ts_fn) = cx.user_fns.get(&to_str_fn) {
                let s = cx
                    .builder
                    .build_direct_call(ts_fn, &[val.into()], "to_str")
                    .expect("call")
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                Ok(s)
            } else {
                let ptr = cx.create_string(&format!("<{sname}>"))?;
                Ok(ptr.into())
            }
        }
        _ => {
            let ptr = cx.create_string("<value>")?;
            Ok(ptr.into())
        }
    }
}

/// Widen a value to i64 for uniform heap storage.
fn widen_for_storage<'a, 'ctx>(
    cx: &Ctx<'a, 'ctx>,
    val: BasicValueEnum<'ctx>,
) -> BasicValueEnum<'ctx> {
    match val {
        BasicValueEnum::IntValue(iv) => {
            if iv.get_type().get_bit_width() < 64 {
                cx.builder
                    .build_int_s_extend(iv, cx.context.i64_type(), "widen")
                    .expect("build_int_s_extend failed")
                    .into()
            } else {
                val
            }
        }
        BasicValueEnum::FloatValue(fv) => {
            // bitcast f64 -> i64 for storage
            cx.builder
                .build_bit_cast(fv, cx.context.i64_type(), "f2i")
                .expect("build_bitcast failed")
        }
        BasicValueEnum::PointerValue(pv) => {
            // ptr -> i64 for storage in uniform-width arrays/structs
            cx.builder
                .build_ptr_to_int(pv, cx.context.i64_type(), "ptr2i")
                .expect("build_ptr_to_int failed")
                .into()
        }
        _ => val,
    }
}

/// Convert an integer value to a pointer if it's an i64 (for channel/mutex/hashmap operations).
fn int_to_ptr_if_needed<'a, 'ctx>(
    cx: &Ctx<'a, 'ctx>,
    val: BasicValueEnum<'ctx>,
) -> PointerValue<'ctx> {
    match val {
        BasicValueEnum::PointerValue(pv) => pv,
        BasicValueEnum::IntValue(iv) => cx
            .builder
            .build_int_to_ptr(iv, cx.context.ptr_type(AddressSpace::default()), "i2ptr")
            .expect("int_to_ptr failed"),
        _ => cx.context.ptr_type(AddressSpace::default()).const_null(),
    }
}

/// Narrow a value from i64 storage back to its actual type.
fn narrow_from_storage<'a, 'ctx>(
    cx: &Ctx<'a, 'ctx>,
    val: BasicValueEnum<'ctx>,
    tty: &TurboTy,
) -> BasicValueEnum<'ctx> {
    match tty {
        TurboTy::Bool => {
            let iv = val.into_int_value();
            cx.builder
                .build_int_truncate(iv, cx.context.i8_type(), "narrow_bool")
                .expect("build_int_truncate failed")
                .into()
        }
        TurboTy::Float => {
            let iv = val.into_int_value();
            cx.builder
                .build_bit_cast(iv, cx.context.f64_type(), "i2f")
                .expect("build_bitcast failed")
        }
        TurboTy::Str
        | TurboTy::Array(_)
        | TurboTy::Struct(_)
        | TurboTy::Result(_, _)
        | TurboTy::Optional(_)
        | TurboTy::Agent(_)
        | TurboTy::Future(_) => {
            // i64 -> ptr via inttoptr
            let iv = val.into_int_value();
            cx.builder
                .build_int_to_ptr(iv, cx.context.ptr_type(AddressSpace::default()), "i2ptr")
                .expect("build_int_to_pointer failed")
                .into()
        }
        _ => val, // Int, Enum, etc. stay as i64
    }
}

/// Coerce argument value to match the expected LLVM type.
fn coerce_arg<'a, 'ctx>(
    cx: &Ctx<'a, 'ctx>,
    val: BasicValueEnum<'ctx>,
    expected: BasicTypeEnum<'ctx>,
) -> BasicValueEnum<'ctx> {
    let actual = val.get_type();
    if actual == expected {
        return val;
    }

    // Int width mismatch
    if let (BasicTypeEnum::IntType(actual_int), BasicTypeEnum::IntType(expected_int)) =
        (actual, expected)
    {
        if actual_int.get_bit_width() < expected_int.get_bit_width() {
            return cx
                .builder
                .build_int_s_extend(val.into_int_value(), expected_int, "coerce_ext")
                .expect("build_int_s_extend failed")
                .into();
        }
        if actual_int.get_bit_width() > expected_int.get_bit_width() {
            return cx
                .builder
                .build_int_truncate(val.into_int_value(), expected_int, "coerce_trunc")
                .expect("build_int_truncate failed")
                .into();
        }
    }

    // Pointer to int coercion
    if let (BasicTypeEnum::PointerType(_), BasicTypeEnum::IntType(expected_int)) =
        (actual, expected)
    {
        return cx
            .builder
            .build_ptr_to_int(val.into_pointer_value(), expected_int, "coerce_ptr2int")
            .expect("build_ptr_to_int failed")
            .into();
    }

    // Int to pointer coercion
    if let (BasicTypeEnum::IntType(_), BasicTypeEnum::PointerType(expected_ptr)) =
        (actual, expected)
    {
        return cx
            .builder
            .build_int_to_ptr(val.into_int_value(), expected_ptr, "coerce_int2ptr")
            .expect("build_int_to_ptr failed")
            .into();
    }

    val
}
