//! Optional shared C observer. It never owns or changes program allocations.
use std::ffi::c_void;

extern "C" {
    fn turbo_profile_start_if_requested() -> i32;
    fn turbo_profile_alloc(raw: *const c_void, total: usize, header: usize, arena: *const c_void);
    fn turbo_profile_free(raw: *const c_void);
    fn turbo_profile_rc(operation: i32);
    fn turbo_profile_finish();
    fn turbo_profile_end(out: *mut c_void);
}

pub(crate) fn allocation(raw: *mut u8, total: usize) {
    unsafe { turbo_profile_alloc(raw.cast(), total, 16, std::ptr::null()) }
}
pub(crate) fn deallocation(raw: *mut u8) {
    unsafe { turbo_profile_free(raw.cast()) }
}
pub(crate) fn rc(operation: i32) {
    unsafe { turbo_profile_rc(operation) }
}

pub(crate) struct Session {
    owned: bool,
}
impl Session {
    pub(crate) fn start() -> Self {
        Self {
            owned: unsafe { turbo_profile_start_if_requested() != 0 },
        }
    }
    pub(crate) fn finish(mut self) {
        if self.owned {
            unsafe { turbo_profile_finish() }
            self.owned = false;
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if self.owned {
            // A failed entry call is not a completed profile. Clean observer
            // bookkeeping without emitting a successful-return record.
            unsafe { turbo_profile_end(std::ptr::null_mut()) }
        }
    }
}
