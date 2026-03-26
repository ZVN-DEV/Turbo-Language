use cranelift::prelude::*;
use cranelift::prelude::isa::CallConv;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{DataDescription, FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use std::collections::HashMap;
use std::path::Path;
use turbo_ast::*;

#[derive(Debug)]
pub struct CodegenError {
    pub message: String,
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "codegen error: {}", self.message)
    }
}

impl std::error::Error for CodegenError {}

/// Turbo-level type tag — needed because on ARM64 ptr_type == I64,
/// so Cranelift IR types alone can't distinguish strings from ints.
#[derive(Debug, Clone, Copy, PartialEq)]
enum TurboTy {
    Int,
    Float,
    Bool,
    Str,
    Unit,
}

fn turbo_ty_from_type_expr(te: &TypeExpr) -> TurboTy {
    match te {
        TypeExpr::Named(name) => match name.as_str() {
            "i32" | "i64" | "u32" | "u64" => TurboTy::Int,
            "f32" | "f64" => TurboTy::Float,
            "bool" => TurboTy::Bool,
            "str" => TurboTy::Str,
            _ => TurboTy::Unit,
        },
        TypeExpr::Unit => TurboTy::Unit,
    }
}

/// Compiled value with its Turbo type.
type Typed = (Value, TurboTy);
/// Optional compiled value (None = unit).
type MaybeTyped = Option<Typed>;

// ── Runtime functions linked into JIT ───────────────────────────────

extern "C" fn rt_print_str(s: *const u8) {
    if s.is_null() {
        println!();
        return;
    }
    let cstr = unsafe { std::ffi::CStr::from_ptr(s as *const std::ffi::c_char) };
    if let Ok(string) = cstr.to_str() {
        println!("{}", string);
    }
}

extern "C" fn rt_print_i64(n: i64) {
    println!("{}", n);
}

extern "C" fn rt_print_f64(n: f64) {
    println!("{}", n);
}

extern "C" fn rt_print_bool(b: i8) {
    println!("{}", if b != 0 { "true" } else { "false" });
}

extern "C" fn rt_panic(msg: *const u8) {
    if !msg.is_null() {
        let cstr = unsafe { std::ffi::CStr::from_ptr(msg as *const std::ffi::c_char) };
        if let Ok(s) = cstr.to_str() {
            eprintln!("panic: {}", s);
        }
    } else {
        eprintln!("panic: explicit panic");
    }
    std::process::exit(1);
}

extern "C" fn rt_assert_fail(msg: *const u8) {
    if !msg.is_null() {
        let cstr = unsafe { std::ffi::CStr::from_ptr(msg as *const std::ffi::c_char) };
        if let Ok(s) = cstr.to_str() {
            eprintln!("assertion failed: {}", s);
        }
    } else {
        eprintln!("assertion failed");
    }
    std::process::exit(1);
}

extern "C" fn rt_div_by_zero() {
    eprintln!("runtime error: division by zero");
    std::process::exit(1);
}

extern "C" fn rt_int_overflow() {
    eprintln!("runtime error: integer overflow");
    std::process::exit(1);
}

// ── Runtime C source for AOT linking ────────────────────────────────

const RUNTIME_C: &str = include_str!("../runtime/turbo_rt.c");

// ── Codegen context (generic over Module type) ──────────────────────

struct Ctx<'a, M: Module> {
    builder: FunctionBuilder<'a>,
    module: &'a mut M,
    user_fns: &'a HashMap<String, FuncId>,
    fn_ret_types: &'a HashMap<String, TurboTy>,
    rt_fns: &'a HashMap<String, FuncId>,
    vars: HashMap<String, (Variable, types::Type, TurboTy)>,
    next_var: usize,
    data_desc: &'a mut DataDescription,
    string_counter: &'a mut usize,
    ptr_type: types::Type,
}

impl<'a, M: Module> Ctx<'a, M> {
    fn fresh_var(&mut self, cl_ty: types::Type, turbo_ty: TurboTy) -> Variable {
        let var = Variable::new(self.next_var);
        self.next_var += 1;
        self.builder.declare_var(var, cl_ty);
        let _ = turbo_ty; // used by caller
        var
    }

    fn create_string(&mut self, s: &str) -> Result<Value, CodegenError> {
        if s.contains('\0') {
            return Err(CodegenError {
                message: "string literal contains null byte, which is not supported".to_string(),
            });
        }

        let name = format!(".str.{}", *self.string_counter);
        *self.string_counter += 1;

        let data_id = self.module.declare_data(&name, Linkage::Local, false, false)
            .map_err(|e| CodegenError { message: e.to_string() })?;

        self.data_desc.clear();
        let mut bytes = s.as_bytes().to_vec();
        bytes.push(0);
        self.data_desc.define(bytes.into_boxed_slice());

        self.module.define_data(data_id, self.data_desc)
            .map_err(|e| CodegenError { message: e.to_string() })?;

        let data_ref = self.module.declare_data_in_func(data_id, self.builder.func);
        let ptr = self.builder.ins().global_value(self.ptr_type, data_ref);
        Ok(ptr)
    }

    fn rt_call(&mut self, name: &str, args: &[Value]) {
        let fid = self.rt_fns[name];
        let fref = self.module.declare_func_in_func(fid, self.builder.func);
        self.builder.ins().call(fref, args);
    }
}

// ── Public entry points ─────────────────────────────────────────────

pub fn jit_run(ast_module: &turbo_ast::Module) -> Result<(), CodegenError> {
    let mut flag_builder = settings::builder();
    flag_builder.set("use_colocated_libcalls", "false").unwrap();
    flag_builder.set("is_pic", "false").unwrap();
    flag_builder.set("opt_level", "speed").unwrap();

    let isa_builder = cranelift_native::builder()
        .map_err(|e| CodegenError { message: e.to_string() })?;
    let isa = isa_builder
        .finish(settings::Flags::new(flag_builder))
        .map_err(|e| CodegenError { message: e.to_string() })?;

    let ptr_type = isa.pointer_type();

    let mut jit_builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());

    // Link runtime functions as symbols
    jit_builder.symbol("rt_print_str", rt_print_str as *const u8);
    jit_builder.symbol("rt_print_i64", rt_print_i64 as *const u8);
    jit_builder.symbol("rt_print_f64", rt_print_f64 as *const u8);
    jit_builder.symbol("rt_print_bool", rt_print_bool as *const u8);
    jit_builder.symbol("rt_panic", rt_panic as *const u8);
    jit_builder.symbol("rt_assert_fail", rt_assert_fail as *const u8);
    jit_builder.symbol("rt_div_by_zero", rt_div_by_zero as *const u8);
    jit_builder.symbol("rt_int_overflow", rt_int_overflow as *const u8);

    let mut module = JITModule::new(jit_builder);
    let user_fns = compile_module(&mut module, ast_module, ptr_type, Linkage::Local, false)?;

    module.finalize_definitions()
        .map_err(|e| CodegenError { message: e.to_string() })?;

    let main_id = user_fns.get("main")
        .ok_or_else(|| CodegenError { message: "no `main` function found".to_string() })?;
    let main_ptr = module.get_finalized_function(*main_id);
    let main_fn: fn() = unsafe { std::mem::transmute(main_ptr) };
    main_fn();

    Ok(())
}

pub fn aot_compile(
    ast_module: &turbo_ast::Module,
    output_path: &Path,
    optimize: bool,
) -> Result<(), CodegenError> {
    let mut flag_builder = settings::builder();
    flag_builder.set("use_colocated_libcalls", "false").unwrap();
    flag_builder.set("is_pic", "true").unwrap(); // Required for AOT linking on macOS
    if optimize {
        flag_builder.set("opt_level", "speed").unwrap();
    }

    let isa_builder = cranelift_native::builder()
        .map_err(|e| CodegenError { message: e.to_string() })?;
    let isa = isa_builder
        .finish(settings::Flags::new(flag_builder))
        .map_err(|e| CodegenError { message: e.to_string() })?;

    let ptr_type = isa.pointer_type();

    let obj_builder = ObjectBuilder::new(
        isa,
        "turbo_module",
        cranelift_module::default_libcall_names(),
    ).map_err(|e| CodegenError { message: e.to_string() })?;

    let mut module = ObjectModule::new(obj_builder);
    compile_module(&mut module, ast_module, ptr_type, Linkage::Export, true)?;

    let product = module.finish();
    let obj_bytes = product.emit()
        .map_err(|e| CodegenError { message: format!("failed to emit object: {e}") })?;

    // Write object file and runtime to temp, then link with cc
    let tmp_dir = std::env::temp_dir().join(format!("turbo_aot_{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir)
        .map_err(|e| CodegenError { message: format!("failed to create temp dir: {e}") })?;

    let obj_path = tmp_dir.join("turbo.o");
    let rt_path = tmp_dir.join("turbo_rt.c");

    std::fs::write(&obj_path, &obj_bytes)
        .map_err(|e| CodegenError { message: format!("failed to write object file: {e}") })?;
    std::fs::write(&rt_path, RUNTIME_C)
        .map_err(|e| CodegenError { message: format!("failed to write runtime: {e}") })?;

    let output = std::process::Command::new("cc")
        .arg(&rt_path)
        .arg(&obj_path)
        .arg("-o")
        .arg(output_path)
        .output()
        .map_err(|e| CodegenError { message: format!("failed to run linker: {e}") })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CodegenError {
            message: format!("linker failed: {stderr}"),
        });
    }

    // Clean up temp directory and all files within
    let _ = std::fs::remove_dir_all(&tmp_dir);

    Ok(())
}

// ── Shared module compilation ───────────────────────────────────────

fn compile_module<M: Module>(
    module: &mut M,
    ast_module: &turbo_ast::Module,
    ptr_type: types::Type,
    main_linkage: Linkage,
    rename_main: bool,
) -> Result<HashMap<String, FuncId>, CodegenError> {
    // Declare runtime functions
    let mut rt_fns: HashMap<String, FuncId> = HashMap::new();
    declare_rt_fn(module, &mut rt_fns, "rt_print_str", &[ptr_type], None)?;
    declare_rt_fn(module, &mut rt_fns, "rt_print_i64", &[types::I64], None)?;
    declare_rt_fn(module, &mut rt_fns, "rt_print_f64", &[types::F64], None)?;
    declare_rt_fn(module, &mut rt_fns, "rt_print_bool", &[types::I8], None)?;
    declare_rt_fn(module, &mut rt_fns, "rt_panic", &[ptr_type], None)?;
    declare_rt_fn(module, &mut rt_fns, "rt_assert_fail", &[ptr_type], None)?;
    declare_rt_fn(module, &mut rt_fns, "rt_div_by_zero", &[], None)?;
    declare_rt_fn(module, &mut rt_fns, "rt_int_overflow", &[], None)?;

    // Declare all user functions + build return type map
    let mut user_fns: HashMap<String, FuncId> = HashMap::new();
    let mut fn_ret_types: HashMap<String, TurboTy> = HashMap::new();

    for item in &ast_module.items {
        let Item::Function(f) = &item.node;
        let mut sig = module.make_signature();
        // Use fast calling convention for internal functions (not main)
        // — reduces prologue/epilogue overhead on the hot recursive path
        if f.name != "main" {
            sig.call_conv = CallConv::Fast;
        }
        for param in &f.params {
            sig.params.push(AbiParam::new(resolve_cl_type(&param.ty.node, ptr_type)?));
        }
        let ret_turbo = if let Some(ret_ty) = &f.return_type {
            let cl = resolve_cl_type(&ret_ty.node, ptr_type)?;
            sig.returns.push(AbiParam::new(cl));
            turbo_ty_from_type_expr(&ret_ty.node)
        } else {
            TurboTy::Unit
        };
        let linkage = if f.name == "main" { main_linkage } else { Linkage::Local };
        let sym_name = if f.name == "main" && rename_main { "turbo_main" } else { &f.name };
        let id = module.declare_function(sym_name, linkage, &sig)
            .map_err(|e| CodegenError { message: e.to_string() })?;
        user_fns.insert(f.name.clone(), id);
        fn_ret_types.insert(f.name.clone(), ret_turbo);
    }

    // Define all user functions
    let mut cl_ctx = module.make_context();
    let mut data_desc = DataDescription::new();
    let mut string_counter: usize = 0;

    for item in &ast_module.items {
        let Item::Function(f) = &item.node;
        let func_id = user_fns[&f.name];

        cl_ctx.func.signature = module.make_signature();
        if f.name != "main" {
            cl_ctx.func.signature.call_conv = CallConv::Fast;
        }
        for param in &f.params {
            cl_ctx.func.signature.params.push(AbiParam::new(resolve_cl_type(&param.ty.node, ptr_type)?));
        }
        if let Some(ret_ty) = &f.return_type {
            cl_ctx.func.signature.returns.push(AbiParam::new(resolve_cl_type(&ret_ty.node, ptr_type)?));
        }

        let mut fn_ctx = FunctionBuilderContext::new();
        {
            let builder = FunctionBuilder::new(&mut cl_ctx.func, &mut fn_ctx);
            let mut cx = Ctx {
                builder,
                module,
                user_fns: &user_fns,
                fn_ret_types: &fn_ret_types,
                rt_fns: &rt_fns,
                vars: HashMap::new(),
                next_var: 0,
                data_desc: &mut data_desc,
                string_counter: &mut string_counter,
                ptr_type,
            };

            let entry = cx.builder.create_block();
            cx.builder.append_block_params_for_function_params(entry);
            cx.builder.switch_to_block(entry);
            cx.builder.seal_block(entry);

            // Ensure the entry block is always in the layout, even if the
            // function body is empty (no instructions). Without this,
            // FunctionBuilder::is_unreachable() misidentifies the entry block
            // as unreachable because layout.entry_block() returns None.
            cx.builder.ensure_inserted_block();

            // Define parameters as variables
            for (i, param) in f.params.iter().enumerate() {
                let cl_ty = resolve_cl_type(&param.ty.node, ptr_type)?;
                let turbo_ty = turbo_ty_from_type_expr(&param.ty.node);
                let var = cx.fresh_var(cl_ty, turbo_ty);
                let val = cx.builder.block_params(entry)[i];
                cx.builder.def_var(var, val);
                cx.vars.insert(param.name.clone(), (var, cl_ty, turbo_ty));
            }

            let result = compile_expr(&mut cx, &f.body)?;

            if !cx.builder.is_unreachable() {
                if f.return_type.is_some() {
                    if let Some((val, _)) = result {
                        cx.builder.ins().return_(&[val]);
                    } else {
                        // Function claims to return a value but body returns unit.
                        // This should have been caught by sema; emit a trap as a safety net.
                        cx.builder.ins().trap(TrapCode::unwrap_user(1));
                    }
                } else {
                    cx.builder.ins().return_(&[]);
                }
            }

            cx.builder.finalize();
        }

        module.define_function(func_id, &mut cl_ctx)
            .map_err(|e| CodegenError { message: e.to_string() })?;
        module.clear_context(&mut cl_ctx);
    }

    Ok(user_fns)
}

// ── Helpers ─────────────────────────────────────────────────────────

fn declare_rt_fn<M: Module>(
    module: &mut M,
    rt_fns: &mut HashMap<String, FuncId>,
    name: &str,
    params: &[types::Type],
    ret: Option<types::Type>,
) -> Result<(), CodegenError> {
    let mut sig = module.make_signature();
    for &p in params {
        sig.params.push(AbiParam::new(p));
    }
    if let Some(r) = ret {
        sig.returns.push(AbiParam::new(r));
    }
    let id = module.declare_function(name, Linkage::Import, &sig)
        .map_err(|e| CodegenError { message: e.to_string() })?;
    rt_fns.insert(name.to_string(), id);
    Ok(())
}

fn resolve_cl_type(ty: &TypeExpr, ptr_type: types::Type) -> Result<types::Type, CodegenError> {
    match ty {
        TypeExpr::Named(name) => match name.as_str() {
            "i32" => Ok(types::I32),
            "i64" => Ok(types::I64),
            "u32" => Ok(types::I32),
            "u64" => Ok(types::I64),
            "f32" => Ok(types::F32),
            "f64" => Ok(types::F64),
            "bool" => Ok(types::I8),
            "str" => Ok(ptr_type),
            _ => Err(CodegenError { message: format!("unknown type: {name}") }),
        },
        TypeExpr::Unit => Err(CodegenError { message: "unit type has no runtime representation".to_string() }),
    }
}

// ── Expression compilation ──────────────────────────────────────────

fn compile_expr<M: Module>(cx: &mut Ctx<'_, M>, expr: &Spanned<Expr>) -> Result<MaybeTyped, CodegenError> {
    match &expr.node {
        Expr::IntLit(n) => {
            let val = cx.builder.ins().iconst(types::I64, *n);
            Ok(Some((val, TurboTy::Int)))
        }
        Expr::FloatLit(f) => {
            let val = cx.builder.ins().f64const(*f);
            Ok(Some((val, TurboTy::Float)))
        }
        Expr::BoolLit(b) => {
            let val = cx.builder.ins().iconst(types::I8, *b as i64);
            Ok(Some((val, TurboTy::Bool)))
        }
        Expr::StringLit(s) => {
            let ptr = cx.create_string(s)?;
            Ok(Some((ptr, TurboTy::Str)))
        }
        Expr::Unit => Ok(None),

        Expr::Ident(name) => {
            let (var, _cl_ty, turbo_ty) = cx.vars.get(name)
                .ok_or_else(|| CodegenError { message: format!("undefined variable: {name}") })?;
            let turbo_ty = *turbo_ty;
            let val = cx.builder.use_var(*var);
            Ok(Some((val, turbo_ty)))
        }

        Expr::BinaryOp { left, op, right } => {
            // Short-circuit for && and ||
            if *op == BinOp::And || *op == BinOp::Or {
                return compile_short_circuit(cx, left, *op, right);
            }

            let (lhs, lhs_tty) = compile_expr(cx, left)?.unwrap();
            let (rhs, _) = compile_expr(cx, right)?.unwrap();
            let result = compile_binop(cx, lhs, *op, rhs)?;

            // Comparison/logical ops produce Bool, arithmetic preserves input type
            let result_tty = match op {
                BinOp::Eq | BinOp::NotEq | BinOp::Less | BinOp::LessEq
                | BinOp::Greater | BinOp::GreaterEq | BinOp::And | BinOp::Or => TurboTy::Bool,
                _ => lhs_tty,
            };
            Ok(Some((result, result_tty)))
        }

        Expr::UnaryOp { op, expr: inner } => {
            let (val, tty) = compile_expr(cx, inner)?.unwrap();
            let result = match op {
                UnaryOp::Neg => {
                    let ty = cx.builder.func.dfg.value_type(val);
                    if ty.is_float() {
                        cx.builder.ins().fneg(val)
                    } else {
                        cx.builder.ins().ineg(val)
                    }
                }
                UnaryOp::Not => {
                    let one = cx.builder.ins().iconst(types::I8, 1);
                    cx.builder.ins().bxor(val, one)
                }
            };
            let result_tty = match op {
                UnaryOp::Not => TurboTy::Bool,
                UnaryOp::Neg => tty,
            };
            Ok(Some((result, result_tty)))
        }

        Expr::Call { callee, args } => compile_call(cx, callee, args),

        Expr::If { condition, then_branch, else_branch } => {
            compile_if(cx, condition, then_branch, else_branch.as_deref())
        }

        Expr::Block { stmts, tail_expr } => {
            for stmt in stmts {
                compile_stmt(cx, stmt)?;
            }
            if let Some(tail) = tail_expr {
                compile_expr(cx, tail)
            } else {
                Ok(None)
            }
        }

        Expr::Assign { target, value } => {
            let (val, tty) = compile_expr(cx, value)?.unwrap();
            let (var, _, _) = cx.vars.get(target)
                .ok_or_else(|| CodegenError { message: format!("undefined variable: {target}") })?;
            let var = *var;
            cx.builder.def_var(var, val);
            // Update the turbo type in case it changed (shouldn't in Phase 1, but safe)
            if let Some(entry) = cx.vars.get_mut(target) {
                entry.2 = tty;
            }
            Ok(None)
        }

        Expr::CompoundAssign { target, op, value } => {
            let (rhs, _) = compile_expr(cx, value)?.unwrap();
            let (var, _, _) = cx.vars.get(target)
                .ok_or_else(|| CodegenError { message: format!("undefined variable: {target}") })?;
            let var = *var;
            let lhs = cx.builder.use_var(var);
            let result = compile_binop(cx, lhs, *op, rhs)?;
            cx.builder.def_var(var, result);
            Ok(None)
        }

        Expr::While { condition, body } => compile_while(cx, condition, body),
    }
}

// ── Statement compilation ───────────────────────────────────────────

fn compile_stmt<M: Module>(cx: &mut Ctx<'_, M>, stmt: &Spanned<Stmt>) -> Result<(), CodegenError> {
    match &stmt.node {
        Stmt::Let { name, value, .. } => {
            let result = compile_expr(cx, value)?;
            let (cl_ty, turbo_ty, val) = if let Some((v, tty)) = result {
                (cx.builder.func.dfg.value_type(v), tty, Some(v))
            } else {
                (types::I64, TurboTy::Unit, None)
            };
            let var = Variable::new(cx.next_var);
            cx.next_var += 1;
            cx.builder.declare_var(var, cl_ty);
            if let Some(v) = val {
                cx.builder.def_var(var, v);
            }
            cx.vars.insert(name.clone(), (var, cl_ty, turbo_ty));
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
                    cx.builder.ins().return_(&[v]);
                } else {
                    cx.builder.ins().return_(&[]);
                }
            } else {
                cx.builder.ins().return_(&[]);
            }
            let new_block = cx.builder.create_block();
            cx.builder.switch_to_block(new_block);
            cx.builder.seal_block(new_block);
            Ok(())
        }
    }
}

// ── Binary operations ───────────────────────────────────────────────

fn compile_binop<M: Module>(
    cx: &mut Ctx<'_, M>,
    lhs: Value,
    op: BinOp,
    rhs: Value,
) -> Result<Value, CodegenError> {
    let lhs_ty = cx.builder.func.dfg.value_type(lhs);

    if lhs_ty.is_float() {
        let result = match op {
            BinOp::Add => cx.builder.ins().fadd(lhs, rhs),
            BinOp::Sub => cx.builder.ins().fsub(lhs, rhs),
            BinOp::Mul => cx.builder.ins().fmul(lhs, rhs),
            BinOp::Div => cx.builder.ins().fdiv(lhs, rhs),
            BinOp::Eq => cx.builder.ins().fcmp(FloatCC::Equal, lhs, rhs),
            BinOp::NotEq => cx.builder.ins().fcmp(FloatCC::NotEqual, lhs, rhs),
            BinOp::Less => cx.builder.ins().fcmp(FloatCC::LessThan, lhs, rhs),
            BinOp::LessEq => cx.builder.ins().fcmp(FloatCC::LessThanOrEqual, lhs, rhs),
            BinOp::Greater => cx.builder.ins().fcmp(FloatCC::GreaterThan, lhs, rhs),
            BinOp::GreaterEq => cx.builder.ins().fcmp(FloatCC::GreaterThanOrEqual, lhs, rhs),
            _ => return Err(CodegenError { message: format!("unsupported float op: {op:?}") }),
        };
        Ok(result)
    } else {
        // Widen mismatched integer widths
        let rhs_ty = cx.builder.func.dfg.value_type(rhs);
        let (lhs, rhs) = if lhs_ty.bits() != rhs_ty.bits() {
            let target = if lhs_ty.bits() > rhs_ty.bits() { lhs_ty } else { rhs_ty };
            let lhs = if lhs_ty.bits() < target.bits() {
                cx.builder.ins().sextend(target, lhs)
            } else { lhs };
            let rhs = if rhs_ty.bits() < target.bits() {
                cx.builder.ins().sextend(target, rhs)
            } else { rhs };
            (lhs, rhs)
        } else {
            (lhs, rhs)
        };

        let result = match op {
            BinOp::Add => cx.builder.ins().iadd(lhs, rhs),
            BinOp::Sub => cx.builder.ins().isub(lhs, rhs),
            BinOp::Mul => cx.builder.ins().imul(lhs, rhs),
            BinOp::Div | BinOp::Mod => {
                emit_div_zero_check(cx, rhs);
                emit_int_overflow_check(cx, lhs, rhs);
                if op == BinOp::Div {
                    cx.builder.ins().sdiv(lhs, rhs)
                } else {
                    cx.builder.ins().srem(lhs, rhs)
                }
            }
            BinOp::Eq => cx.builder.ins().icmp(IntCC::Equal, lhs, rhs),
            BinOp::NotEq => cx.builder.ins().icmp(IntCC::NotEqual, lhs, rhs),
            BinOp::Less => cx.builder.ins().icmp(IntCC::SignedLessThan, lhs, rhs),
            BinOp::LessEq => cx.builder.ins().icmp(IntCC::SignedLessThanOrEqual, lhs, rhs),
            BinOp::Greater => cx.builder.ins().icmp(IntCC::SignedGreaterThan, lhs, rhs),
            BinOp::GreaterEq => cx.builder.ins().icmp(IntCC::SignedGreaterThanOrEqual, lhs, rhs),
            BinOp::And => cx.builder.ins().band(lhs, rhs),
            BinOp::Or => cx.builder.ins().bor(lhs, rhs),
        };
        Ok(result)
    }
}

fn emit_div_zero_check<M: Module>(cx: &mut Ctx<'_, M>, divisor: Value) {
    let ty = cx.builder.func.dfg.value_type(divisor);
    let zero = cx.builder.ins().iconst(ty, 0);
    let is_zero = cx.builder.ins().icmp(IntCC::Equal, divisor, zero);

    let trap_block = cx.builder.create_block();
    let ok_block = cx.builder.create_block();

    cx.builder.ins().brif(is_zero, trap_block, &[], ok_block, &[]);

    cx.builder.switch_to_block(trap_block);
    cx.builder.seal_block(trap_block);
    cx.rt_call("rt_div_by_zero", &[]);
    cx.builder.ins().trap(TrapCode::unwrap_user(1));

    cx.builder.switch_to_block(ok_block);
    cx.builder.seal_block(ok_block);
}

fn emit_int_overflow_check<M: Module>(cx: &mut Ctx<'_, M>, dividend: Value, divisor: Value) {
    let ty = cx.builder.func.dfg.value_type(dividend);

    // Check if divisor is -1
    let neg_one = cx.builder.ins().iconst(ty, -1i64);
    let is_neg_one = cx.builder.ins().icmp(IntCC::Equal, divisor, neg_one);

    // Check if dividend is INT_MIN (0x8000...0)
    let int_min = cx.builder.ins().iconst(ty, i64::MIN);
    let is_int_min = cx.builder.ins().icmp(IntCC::Equal, dividend, int_min);

    // Both conditions must be true for overflow
    let is_overflow = cx.builder.ins().band(is_neg_one, is_int_min);

    let trap_block = cx.builder.create_block();
    let ok_block = cx.builder.create_block();

    cx.builder.ins().brif(is_overflow, trap_block, &[], ok_block, &[]);

    cx.builder.switch_to_block(trap_block);
    cx.builder.seal_block(trap_block);
    cx.rt_call("rt_int_overflow", &[]);
    cx.builder.ins().trap(TrapCode::unwrap_user(1));

    cx.builder.switch_to_block(ok_block);
    cx.builder.seal_block(ok_block);
}

// ── Short-circuit && / || ───────────────────────────────────────────

fn compile_short_circuit<M: Module>(
    cx: &mut Ctx<'_, M>,
    left: &Spanned<Expr>,
    op: BinOp,
    right: &Spanned<Expr>,
) -> Result<MaybeTyped, CodegenError> {
    let (lhs, _) = compile_expr(cx, left)?.unwrap();

    let lhs_ty = cx.builder.func.dfg.value_type(lhs);
    let lhs_bool = {
        let zero = cx.builder.ins().iconst(lhs_ty, 0);
        cx.builder.ins().icmp(IntCC::NotEqual, lhs, zero)
    };

    let eval_rhs_block = cx.builder.create_block();
    let merge_block = cx.builder.create_block();
    cx.builder.append_block_param(merge_block, types::I8);

    match op {
        BinOp::And => {
            let false_val = cx.builder.ins().iconst(types::I8, 0);
            cx.builder.ins().brif(lhs_bool, eval_rhs_block, &[], merge_block, &[false_val]);
        }
        BinOp::Or => {
            let true_val = cx.builder.ins().iconst(types::I8, 1);
            cx.builder.ins().brif(lhs_bool, merge_block, &[true_val], eval_rhs_block, &[]);
        }
        _ => unreachable!(),
    }

    cx.builder.switch_to_block(eval_rhs_block);
    cx.builder.seal_block(eval_rhs_block);
    let (rhs, _) = compile_expr(cx, right)?.unwrap();

    let rhs_ty = cx.builder.func.dfg.value_type(rhs);
    let rhs_as_i8 = if rhs_ty == types::I8 {
        rhs
    } else {
        let zero = cx.builder.ins().iconst(rhs_ty, 0);
        cx.builder.ins().icmp(IntCC::NotEqual, rhs, zero)
    };

    cx.builder.ins().jump(merge_block, &[rhs_as_i8]);

    cx.builder.switch_to_block(merge_block);
    cx.builder.seal_block(merge_block);

    let result = cx.builder.block_params(merge_block)[0];
    Ok(Some((result, TurboTy::Bool)))
}

// ── Function calls ──────────────────────────────────────────────────

fn compile_call<M: Module>(
    cx: &mut Ctx<'_, M>,
    callee: &Spanned<Expr>,
    args: &[Spanned<Expr>],
) -> Result<MaybeTyped, CodegenError> {
    let Expr::Ident(name) = &callee.node else {
        return Err(CodegenError { message: "indirect function calls not yet supported".to_string() });
    };

    match name.as_str() {
        "print" => compile_print(cx, args),
        "panic" => compile_panic(cx, args),
        "assert" => compile_assert(cx, args),
        _ => {
            let func_id = *cx.user_fns.get(name.as_str())
                .ok_or_else(|| CodegenError { message: format!("undefined function: {name}") })?;

            let ret_tty = cx.fn_ret_types.get(name.as_str()).copied().unwrap_or(TurboTy::Unit);

            let func_ref = cx.module.declare_func_in_func(func_id, cx.builder.func);
            let mut arg_values = Vec::new();
            for arg in args {
                if let Some((val, _)) = compile_expr(cx, arg)? {
                    arg_values.push(val);
                }
            }

            let call = cx.builder.ins().call(func_ref, &arg_values);
            let results = cx.builder.inst_results(call);
            if results.is_empty() {
                Ok(None)
            } else {
                Ok(Some((results[0], ret_tty)))
            }
        }
    }
}

fn compile_print<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    if args.is_empty() {
        let ptr = cx.create_string("")?;
        cx.rt_call("rt_print_str", &[ptr]);
        return Ok(None);
    }

    let result = compile_expr(cx, &args[0])?;

    if let Some((v, tty)) = result {
        match tty {
            TurboTy::Str => cx.rt_call("rt_print_str", &[v]),
            TurboTy::Float => cx.rt_call("rt_print_f64", &[v]),
            TurboTy::Bool => cx.rt_call("rt_print_bool", &[v]),
            TurboTy::Int => {
                let ty = cx.builder.func.dfg.value_type(v);
                let v = if ty.bits() < 64 {
                    cx.builder.ins().sextend(types::I64, v)
                } else { v };
                cx.rt_call("rt_print_i64", &[v]);
            }
            TurboTy::Unit => {
                let ptr = cx.create_string("()")?;
                cx.rt_call("rt_print_str", &[ptr]);
            }
        }
    } else {
        let ptr = cx.create_string("()")?;
        cx.rt_call("rt_print_str", &[ptr]);
    }

    Ok(None)
}

fn compile_panic<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    let msg = if !args.is_empty() {
        compile_expr(cx, &args[0])?.unwrap().0
    } else {
        cx.create_string("explicit panic")?
    };

    cx.rt_call("rt_panic", &[msg]);
    cx.builder.ins().trap(TrapCode::unwrap_user(1));

    let new_block = cx.builder.create_block();
    cx.builder.switch_to_block(new_block);
    cx.builder.seal_block(new_block);

    Ok(None)
}

fn compile_assert<M: Module>(cx: &mut Ctx<'_, M>, args: &[Spanned<Expr>]) -> Result<MaybeTyped, CodegenError> {
    if args.is_empty() {
        return Err(CodegenError { message: "assert() requires at least one argument".to_string() });
    }

    let (cond, _) = compile_expr(cx, &args[0])?.unwrap();

    let cond_ty = cx.builder.func.dfg.value_type(cond);
    let cond_bool = {
        let zero = cx.builder.ins().iconst(cond_ty, 0);
        cx.builder.ins().icmp(IntCC::NotEqual, cond, zero)
    };

    let fail_block = cx.builder.create_block();
    let ok_block = cx.builder.create_block();

    cx.builder.ins().brif(cond_bool, ok_block, &[], fail_block, &[]);

    cx.builder.switch_to_block(fail_block);
    cx.builder.seal_block(fail_block);

    let msg = if args.len() > 1 {
        compile_expr(cx, &args[1])?.unwrap().0
    } else {
        cx.create_string("assertion failed")?
    };

    cx.rt_call("rt_assert_fail", &[msg]);
    cx.builder.ins().trap(TrapCode::unwrap_user(1));

    cx.builder.switch_to_block(ok_block);
    cx.builder.seal_block(ok_block);

    Ok(None)
}

// ── If expression ───────────────────────────────────────────────────

fn compile_if<M: Module>(
    cx: &mut Ctx<'_, M>,
    condition: &Spanned<Expr>,
    then_branch: &Spanned<Expr>,
    else_branch: Option<&Spanned<Expr>>,
) -> Result<MaybeTyped, CodegenError> {
    let (cond, _) = compile_expr(cx, condition)?.unwrap();

    let cond_ty = cx.builder.func.dfg.value_type(cond);
    let cond_bool = {
        let zero = cx.builder.ins().iconst(cond_ty, 0);
        cx.builder.ins().icmp(IntCC::NotEqual, cond, zero)
    };

    let then_block = cx.builder.create_block();
    let else_block = cx.builder.create_block();
    let merge_block = cx.builder.create_block();

    cx.builder.ins().brif(cond_bool, then_block, &[], else_block, &[]);

    // Then
    cx.builder.switch_to_block(then_block);
    cx.builder.seal_block(then_block);
    let then_result = compile_expr(cx, then_branch)?;
    let then_needs_jump = !cx.builder.is_unreachable();
    if then_needs_jump {
        if let Some((v, _)) = then_result {
            cx.builder.ins().jump(merge_block, &[v]);
        } else {
            cx.builder.ins().jump(merge_block, &[]);
        }
    }

    // Else
    cx.builder.switch_to_block(else_block);
    cx.builder.seal_block(else_block);
    let else_result = if let Some(else_expr) = else_branch {
        compile_expr(cx, else_expr)?
    } else {
        None
    };
    let else_needs_jump = !cx.builder.is_unreachable();
    if else_needs_jump {
        if let Some((v, _)) = else_result {
            cx.builder.ins().jump(merge_block, &[v]);
        } else {
            cx.builder.ins().jump(merge_block, &[]);
        }
    }

    // Merge
    cx.builder.switch_to_block(merge_block);
    cx.builder.seal_block(merge_block);

    if let (Some((then_val, then_tty)), Some(_)) = (then_result, else_result) {
        let ty = cx.builder.func.dfg.value_type(then_val);
        cx.builder.append_block_param(merge_block, ty);
        let param = cx.builder.block_params(merge_block)[0];
        Ok(Some((param, then_tty)))
    } else {
        Ok(None)
    }
}

// ── While loop ──────────────────────────────────────────────────────

fn compile_while<M: Module>(
    cx: &mut Ctx<'_, M>,
    condition: &Spanned<Expr>,
    body: &Spanned<Expr>,
) -> Result<MaybeTyped, CodegenError> {
    let header_block = cx.builder.create_block();
    let body_block = cx.builder.create_block();
    let exit_block = cx.builder.create_block();

    cx.builder.ins().jump(header_block, &[]);

    // Header: evaluate condition
    cx.builder.switch_to_block(header_block);
    let (cond, _) = compile_expr(cx, condition)?.unwrap();

    let cond_ty = cx.builder.func.dfg.value_type(cond);
    let cond_bool = {
        let zero = cx.builder.ins().iconst(cond_ty, 0);
        cx.builder.ins().icmp(IntCC::NotEqual, cond, zero)
    };

    cx.builder.ins().brif(cond_bool, body_block, &[], exit_block, &[]);

    // Body
    cx.builder.switch_to_block(body_block);
    cx.builder.seal_block(body_block);
    compile_expr(cx, body)?;

    if !cx.builder.is_unreachable() {
        cx.builder.ins().jump(header_block, &[]);
    }

    cx.builder.seal_block(header_block);

    cx.builder.switch_to_block(exit_block);
    cx.builder.seal_block(exit_block);

    Ok(None)
}
