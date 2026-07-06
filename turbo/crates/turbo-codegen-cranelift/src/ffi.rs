use crate::jit::{compile_jit_program, JitProgram};
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::panic::{catch_unwind, AssertUnwindSafe};
use turbo_ast::{Item, Module, TypeExpr};

const NULL_VM_ERROR: &[u8] = b"null Turbo VM\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FfiValueType {
    I64,
    Str,
    Unit,
    Other,
}

#[derive(Clone, Copy, Debug)]
struct FunctionInfo {
    arity: usize,
    ret: FfiValueType,
}

struct HostSymbol {
    name: String,
    ptr: *const u8,
}

pub struct TurboVm {
    program: Option<JitProgram>,
    functions: HashMap<String, FunctionInfo>,
    host_symbols: Vec<HostSymbol>,
    last_error: Option<CString>,
    last_string: Option<CString>,
}

impl TurboVm {
    fn new() -> Self {
        Self {
            program: None,
            functions: HashMap::new(),
            host_symbols: Vec::new(),
            last_error: None,
            last_string: None,
        }
    }

    fn clear_error(&mut self) {
        self.last_error = None;
    }

    fn set_error(&mut self, message: impl Into<String>) {
        self.last_error = Some(cstring_lossy(message.into()));
    }

    fn host_symbol_refs(&self) -> Vec<(&str, *const u8)> {
        self.host_symbols
            .iter()
            .map(|symbol| (symbol.name.as_str(), symbol.ptr))
            .collect()
    }
}

fn cstring_lossy(mut message: String) -> CString {
    if message.contains('\0') {
        message = message.replace('\0', "\\0");
    }
    CString::new(message).expect("interior nulls were replaced")
}

unsafe fn c_str_arg<'a>(ptr: *const c_char, name: &str) -> Result<&'a CStr, String> {
    if ptr.is_null() {
        return Err(format!("{name} is null"));
    }
    Ok(CStr::from_ptr(ptr))
}

fn c_str_to_str<'a>(ptr: *const c_char, name: &str) -> Result<&'a str, String> {
    let cstr = unsafe { c_str_arg(ptr, name)? };
    cstr.to_str()
        .map_err(|_| format!("{name} must be valid UTF-8"))
}

fn parse_checked_source(source: &str) -> Result<(Module, HashMap<String, FunctionInfo>), String> {
    let (tokens, lex_errors) = turbo_lexer::tokenize(source);
    if !lex_errors.is_empty() {
        return Err(format!("lex error: {:?}", lex_errors[0]));
    }

    let (module, parse_errors) = turbo_parser::parse(tokens);
    if !parse_errors.is_empty() {
        return Err(format!("parse error: {}", parse_errors[0]));
    }

    let sema_result = turbo_sema::check_library(&module);
    if !sema_result.errors.is_empty() {
        return Err(format!("semantic error: {}", sema_result.errors[0].message));
    }

    let functions = collect_function_info(&module);
    Ok((module, functions))
}

fn collect_function_info(module: &Module) -> HashMap<String, FunctionInfo> {
    let mut functions = HashMap::new();
    for item in &module.items {
        let Item::Function(function) = &item.node else {
            continue;
        };
        let ret = function
            .return_type
            .as_ref()
            .map(|ret| ffi_type_from_type_expr(&ret.node))
            .unwrap_or(FfiValueType::Unit);
        functions.insert(
            function.name.clone(),
            FunctionInfo {
                arity: function.params.len(),
                ret,
            },
        );
    }
    functions
}

fn ffi_type_from_type_expr(ty: &TypeExpr) -> FfiValueType {
    match ty {
        TypeExpr::Named(name) if name == "int" || name == "i64" => FfiValueType::I64,
        TypeExpr::Named(name) if name == "str" => FfiValueType::Str,
        TypeExpr::Unit => FfiValueType::Unit,
        _ => FfiValueType::Other,
    }
}

fn require_zero_arg_return(
    vm: &TurboVm,
    fn_name: &str,
    expected: FfiValueType,
) -> Result<(), String> {
    let info = vm
        .functions
        .get(fn_name)
        .ok_or_else(|| format!("no function `{fn_name}` found"))?;
    if info.arity != 0 {
        return Err(format!(
            "`{fn_name}` must be a zero-argument function for this libturbo call"
        ));
    }
    if info.ret != expected {
        return Err(format!(
            "`{fn_name}` has an unsupported return type for this libturbo call"
        ));
    }
    Ok(())
}

fn compile_into_vm(vm: &mut TurboVm, source: &str) -> Result<(), String> {
    let (module, functions) = parse_checked_source(source)?;
    let host_symbols = vm.host_symbol_refs();
    let program = compile_jit_program(&module, &host_symbols)
        .map_err(|error| format!("codegen error[{}]: {}", error.code, error.message))?;

    vm.program = Some(program);
    vm.functions = functions;

    if let Some(main) = vm.functions.get("main") {
        if main.arity != 0 || main.ret != FfiValueType::Unit {
            return Err("`main` must be a zero-argument unit function for turbo_eval".to_string());
        }
        let program = vm.program.as_ref().expect("program was just installed");
        program
            .call_zero_arg_void("main")
            .map_err(|error| format!("codegen error[{}]: {}", error.code, error.message))?;
        crate::runtime::rt_arena_reset();
    }

    Ok(())
}

fn register_host_fn(
    vm: &mut TurboVm,
    name: *const c_char,
    fn_ptr: *const c_void,
) -> Result<(), String> {
    if vm.program.is_some() {
        return Err("host functions must be registered before turbo_eval".to_string());
    }
    if fn_ptr.is_null() {
        return Err("host function pointer is null".to_string());
    }
    let name = c_str_to_str(name, "host function name")?;
    if name.is_empty() {
        return Err("host function name is empty".to_string());
    }
    if name.starts_with("rt_") {
        return Err("host function names may not use the reserved `rt_` prefix".to_string());
    }

    vm.host_symbols.retain(|symbol| symbol.name != name);
    vm.host_symbols.push(HostSymbol {
        name: name.to_string(),
        ptr: fn_ptr as *const u8,
    });
    Ok(())
}

#[no_mangle]
pub extern "C" fn turbo_vm_new() -> *mut TurboVm {
    Box::into_raw(Box::new(TurboVm::new()))
}

#[no_mangle]
pub unsafe extern "C" fn turbo_vm_free(vm: *mut TurboVm) {
    if !vm.is_null() {
        drop(Box::from_raw(vm));
    }
}

#[no_mangle]
pub unsafe extern "C" fn turbo_vm_last_error(vm: *const TurboVm) -> *const c_char {
    if vm.is_null() {
        return NULL_VM_ERROR.as_ptr() as *const c_char;
    }
    let vm = &*vm;
    vm.last_error
        .as_ref()
        .map(|error| error.as_ptr())
        .unwrap_or(std::ptr::null())
}

#[no_mangle]
pub unsafe extern "C" fn turbo_vm_register_host_fn(
    vm: *mut TurboVm,
    name: *const c_char,
    fn_ptr: *const c_void,
) -> bool {
    if vm.is_null() {
        return false;
    }
    let vm = &mut *vm;
    match catch_unwind(AssertUnwindSafe(|| register_host_fn(vm, name, fn_ptr))) {
        Ok(Ok(())) => {
            vm.clear_error();
            true
        }
        Ok(Err(message)) => {
            vm.set_error(message);
            false
        }
        Err(_) => {
            vm.set_error("panic while registering host function");
            false
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn turbo_eval(vm: *mut TurboVm, source: *const c_char) -> bool {
    if vm.is_null() {
        return false;
    }
    let vm = &mut *vm;
    match catch_unwind(AssertUnwindSafe(|| {
        let source = c_str_to_str(source, "source")?;
        compile_into_vm(vm, source)
    })) {
        Ok(Ok(())) => {
            vm.clear_error();
            true
        }
        Ok(Err(message)) => {
            vm.set_error(message);
            false
        }
        Err(_) => {
            vm.set_error("panic while evaluating source");
            false
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn turbo_call_i64(
    vm: *mut TurboVm,
    fn_name: *const c_char,
    out: *mut i64,
) -> bool {
    if vm.is_null() {
        return false;
    }
    let vm = &mut *vm;
    match catch_unwind(AssertUnwindSafe(|| {
        if out.is_null() {
            return Err("out pointer is null".to_string());
        }
        let fn_name = c_str_to_str(fn_name, "function name")?;
        require_zero_arg_return(vm, fn_name, FfiValueType::I64)?;
        let program = vm
            .program
            .as_ref()
            .ok_or_else(|| "no Turbo program has been evaluated".to_string())?;
        let value = program
            .call_zero_arg_i64(fn_name)
            .map_err(|error| format!("codegen error[{}]: {}", error.code, error.message))?;
        *out = value;
        crate::runtime::rt_arena_reset();
        Ok(())
    })) {
        Ok(Ok(())) => {
            vm.clear_error();
            true
        }
        Ok(Err(message)) => {
            vm.set_error(message);
            false
        }
        Err(_) => {
            vm.set_error("panic while calling Turbo i64 function");
            false
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn turbo_call_str(vm: *mut TurboVm, fn_name: *const c_char) -> *const c_char {
    if vm.is_null() {
        return std::ptr::null();
    }
    let vm = &mut *vm;
    match catch_unwind(AssertUnwindSafe(|| {
        let fn_name = c_str_to_str(fn_name, "function name")?;
        require_zero_arg_return(vm, fn_name, FfiValueType::Str)?;
        let program = vm
            .program
            .as_ref()
            .ok_or_else(|| "no Turbo program has been evaluated".to_string())?;
        let ptr = program
            .call_zero_arg_str(fn_name)
            .map_err(|error| format!("codegen error[{}]: {}", error.code, error.message))?;
        if ptr.is_null() {
            return Err(format!("`{fn_name}` returned a null string"));
        }

        let value = CStr::from_ptr(ptr as *const c_char).to_owned();
        crate::runtime::rt_arena_reset();
        vm.last_string = Some(value);
        Ok(vm
            .last_string
            .as_ref()
            .expect("last_string was just set")
            .as_ptr())
    })) {
        Ok(Ok(ptr)) => {
            vm.clear_error();
            ptr
        }
        Ok(Err(message)) => {
            vm.set_error(message);
            std::ptr::null()
        }
        Err(_) => {
            vm.set_error("panic while calling Turbo string function");
            std::ptr::null()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static HOST_GREETING: &[u8] = b"hello Turbo from host\0";

    extern "C" fn host_add(value: i64, label: *const c_char) -> i64 {
        let label = unsafe { CStr::from_ptr(label) }.to_str().unwrap();
        assert_eq!(label, "from turbo");
        value + 5
    }

    extern "C" fn host_greet(name: *const c_char) -> *const c_char {
        let name = unsafe { CStr::from_ptr(name) }.to_str().unwrap();
        assert_eq!(name, "Turbo");
        HOST_GREETING.as_ptr() as *const c_char
    }

    fn cstring(value: &str) -> CString {
        CString::new(value).unwrap()
    }

    unsafe fn last_error(vm: *const TurboVm) -> String {
        let ptr = turbo_vm_last_error(vm);
        if ptr.is_null() {
            return String::new();
        }
        CStr::from_ptr(ptr).to_string_lossy().into_owned()
    }

    #[test]
    fn libturbo_eval_registers_host_callbacks_and_calls_turbo_functions() {
        unsafe {
            let vm = turbo_vm_new();
            assert!(!vm.is_null());

            let add = cstring("host_add");
            assert!(turbo_vm_register_host_fn(
                vm,
                add.as_ptr(),
                host_add as *const () as *const c_void,
            ));
            let greet = cstring("host_greet");
            assert!(turbo_vm_register_host_fn(
                vm,
                greet.as_ptr(),
                host_greet as *const () as *const c_void,
            ));

            let source = cstring(
                r#"
@unsafe
extern "C" {
    fn host_add(value: i64, label: str) -> i64
    fn host_greet(name: str) -> str
}

fn answer() -> i64 {
    host_add(37, "from turbo")
}

fn message() -> str {
    host_greet("Turbo")
}

fn main() {
    assert_eq(answer(), 42)
    assert_eq(len(message()), 21)
}
"#,
            );
            assert!(turbo_eval(vm, source.as_ptr()), "{}", last_error(vm));

            let answer_name = cstring("answer");
            let mut answer = 0;
            assert!(turbo_call_i64(vm, answer_name.as_ptr(), &mut answer));
            assert_eq!(answer, 42);

            let message_name = cstring("message");
            let message_ptr = turbo_call_str(vm, message_name.as_ptr());
            assert!(!message_ptr.is_null(), "{}", last_error(vm));
            let message = CStr::from_ptr(message_ptr).to_str().unwrap();
            assert_eq!(message, "hello Turbo from host");

            turbo_vm_free(vm);
        }
    }

    #[test]
    fn libturbo_rejects_wrong_call_shape() {
        unsafe {
            let vm = turbo_vm_new();
            let source = cstring("fn needs_arg(value: i64) -> i64 { value }");
            assert!(turbo_eval(vm, source.as_ptr()), "{}", last_error(vm));

            let fn_name = cstring("needs_arg");
            let mut out = 0;
            assert!(!turbo_call_i64(vm, fn_name.as_ptr(), &mut out));
            assert!(last_error(vm).contains("zero-argument"));

            turbo_vm_free(vm);
        }
    }

    #[test]
    fn libturbo_rejects_host_registration_after_eval() {
        unsafe {
            let vm = turbo_vm_new();
            let source = cstring("fn main() {}");
            assert!(turbo_eval(vm, source.as_ptr()), "{}", last_error(vm));

            let add = cstring("host_add");
            assert!(!turbo_vm_register_host_fn(
                vm,
                add.as_ptr(),
                host_add as *const () as *const c_void,
            ));
            assert_eq!(
                last_error(vm),
                "host functions must be registered before turbo_eval"
            );

            turbo_vm_free(vm);
        }
    }
}
