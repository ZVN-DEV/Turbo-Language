//! Statement compilation: compile_stmt.

use inkwell::types::BasicType;
use turbo_ast::*;

use crate::ctx::Ctx;
use crate::expr::{compile_expr, narrow_from_storage};
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
        Stmt::LetDestructure { fields, value, .. } => {
            // Compile the value expression (should produce a struct pointer)
            let (struct_val, struct_tty) =
                compile_expr(cx, value)?.ok_or_else(|| CodegenError {
                    code: ErrorCode::E0400,
                    message: "destructured value produced no result".to_string(),
                })?;

            let struct_name = match &struct_tty {
                TurboTy::Struct(name) => name.clone(),
                _ => {
                    return Err(CodegenError {
                        code: ErrorCode::E0400,
                        message: "cannot destructure non-struct type".to_string(),
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

            // Structs are heap-allocated: values are pointers. Fields are
            // stored uniformly as i64 and narrowed to their real type on load,
            // mirroring the Cranelift backend.
            let struct_ptr = struct_val.into_pointer_value();
            let i64_ty = cx.context.i64_type();
            let i8_ty = cx.context.i8_type();

            for field_name in fields {
                let field_index = struct_layout
                    .iter()
                    .position(|(n, _)| n == field_name)
                    .ok_or_else(|| CodegenError {
                        code: ErrorCode::E0400,
                        message: format!("struct `{struct_name}` has no field `{field_name}`"),
                    })?;

                let field_tty = struct_layout[field_index].1.clone();
                let offset = (field_index as u64) * 8;

                let field_ptr = unsafe {
                    cx.builder
                        .build_gep(
                            i8_ty,
                            struct_ptr,
                            &[i64_ty.const_int(offset, false)],
                            "destructure_field_ptr",
                        )
                        .expect("build_gep failed")
                };

                let raw = cx
                    .builder
                    .build_load(i64_ty, field_ptr, field_name)
                    .expect("build_load failed");
                let narrowed = narrow_from_storage(cx, raw, &field_tty);

                let alloca = cx.create_entry_block_alloca(narrowed.get_type(), field_name);
                cx.builder
                    .build_store(alloca, narrowed)
                    .expect("build_store failed");
                cx.vars.insert(field_name.clone(), (alloca, field_tty));
            }
            Ok(())
        }
    }
}
