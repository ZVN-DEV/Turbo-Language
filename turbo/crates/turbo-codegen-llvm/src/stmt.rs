//! Statement compilation: compile_stmt.

use inkwell::types::BasicType;
use turbo_ast::*;

use crate::ctx::Ctx;
use crate::expr::compile_expr;
use crate::types::TurboTy;
use crate::CodegenError;

// ── Statement compilation ──────────────────────────────────────────

pub(crate) fn compile_stmt<'a, 'ctx>(
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
