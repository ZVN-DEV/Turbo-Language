use std::ffi::CStr;
use std::os::raw::{c_char, c_double, c_int};

unsafe extern "C" {
    fn snprintf(s: *mut c_char, n: usize, format: *const c_char, ...) -> c_int;
}

fn format_f64(value: f64) -> String {
    let value = if value == 0.0 { 0.0 } else { value };
    let mut buf = [0 as c_char; 64];
    unsafe {
        snprintf(
            buf.as_mut_ptr(),
            buf.len(),
            b"%.15g\0".as_ptr() as *const c_char,
            value as c_double,
        );
        CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
    }
}

fn main() {
    let mut sum = 0.0f64;
    let mut sign = 1.0f64;
    for i in 0..50_000_000i64 {
        sum += sign / (2 * i + 1) as f64;
        sign = -sign;
    }
    println!("{}", format_f64(sum * 4.0));
}
