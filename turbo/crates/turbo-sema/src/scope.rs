//! Scope stack and name resolution helpers used by the semantic checker.
//!
//! This module owns:
//! * `Scope` — a single lexical scope containing `VarInfo` bindings and a
//!   `used_vars` set for unused-variable warnings.
//! * `VarInfo` — per-variable metadata (type, mutability, binding span,
//!   whether it came from a `let`).
//! * `impl Checker` block with `push_scope` / `pop_scope` /
//!   `define_var` / `lookup_var`. The unused-variable warning is emitted
//!   directly from `pop_scope` when a scope is dropped.
//!
//! Kept as a thin module because the checker methods here are purely
//! mechanical — all real type-checking lives in `type_check.rs` and the
//! match-arm exhaustiveness logic lives in `exhaustiveness.rs`.

use std::collections::{HashMap, HashSet};

use turbo_ast::{ErrorCode, Span};

use crate::{Checker, SemaWarning, Ty};

/// Variable info in scope
#[derive(Debug, Clone)]
pub(crate) struct VarInfo {
    pub(crate) ty: Ty,
    pub(crate) mutable: bool,
    /// Span of the let binding (for unused variable warnings)
    pub(crate) span: Span,
    /// Whether this variable was introduced by a `let` statement (as opposed to
    /// function/closure params, match bindings, for-in vars, etc.).
    /// Only `let` bindings emit E0515 unused variable warnings.
    pub(crate) from_let: bool,
}

/// Scope for variable tracking
pub(crate) struct Scope {
    pub(crate) vars: HashMap<String, VarInfo>,
    /// Names of variables that have been referenced in this or child scopes
    pub(crate) used_vars: HashSet<String>,
}

impl Checker {
    // === Scope management ===

    pub(crate) fn push_scope(&mut self) {
        self.scopes.push(Scope {
            vars: HashMap::new(),
            used_vars: HashSet::new(),
        });
    }

    pub(crate) fn pop_scope(&mut self) {
        if let Some(popped) = self.scopes.pop() {
            // Emit unused variable warnings for let bindings in this scope
            for (name, info) in &popped.vars {
                if name.starts_with('_') || name == "self" {
                    continue;
                }
                if info.ty.is_error() || info.span == (0..0) {
                    continue;
                }
                // Only warn for `let` bindings — not for function/closure params,
                // match destructure bindings, if-let bindings, or for-in vars.
                if !info.from_let {
                    continue;
                }
                if !popped.used_vars.contains(name.as_str()) {
                    self.warnings.push(SemaWarning {
                        code: ErrorCode::E0515,
                        message: format!(
                            "unused variable `{name}` — if this is intentional, prefix with an underscore: `_{name}`"
                        ),
                        span: info.span.clone(),
                    });
                }
            }
            // Propagate used_vars to the parent scope so that variables defined
            // in the parent are counted as "used" even if referenced from a child scope.
            if let Some(parent) = self.scopes.last_mut() {
                for name in popped.used_vars {
                    parent.used_vars.insert(name);
                }
            }
        }
    }

    pub(crate) fn define_var(&mut self, name: &str, info: VarInfo, span: &Span) {
        // Check current scope for redefinition (shadowing is OK across scopes)
        if let Some(scope) = self.scopes.last_mut() {
            let mut info = info;
            if info.span.is_empty() {
                info.span = span.clone();
            }
            scope.vars.insert(name.to_string(), info);
        }
    }

    pub(crate) fn lookup_var(&mut self, name: &str) -> Option<VarInfo> {
        let mut found = None;
        for scope in self.scopes.iter().rev() {
            if let Some(info) = scope.vars.get(name) {
                found = Some(info.clone());
                break;
            }
        }
        if found.is_some() {
            // Mark variable as used in the current (innermost) scope
            if let Some(scope) = self.scopes.last_mut() {
                scope.used_vars.insert(name.to_string());
            }
        }
        found
    }
}
