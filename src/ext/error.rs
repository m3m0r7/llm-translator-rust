use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr;

thread_local! {
    static LAST_ERROR: RefCell<Option<String>> = RefCell::new(None);
}

pub(crate) fn set_last_error(message: impl Into<String>) {
    LAST_ERROR.with(|cell| {
        *cell.borrow_mut() = Some(message.into());
    });
}

fn take_last_error() -> Option<String> {
    LAST_ERROR.with(|cell| cell.borrow_mut().take())
}

pub(crate) fn cstr_to_string(value: *const c_char) -> Option<String> {
    if value.is_null() {
        return None;
    }
    unsafe { Some(CStr::from_ptr(value).to_string_lossy().to_string()) }
}

pub(crate) fn string_to_c(value: &str) -> *mut c_char {
    match CString::new(value) {
        Ok(cstr) => cstr.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn llm_ext_free_string(value: *mut c_char) {
    if value.is_null() {
        return;
    }
    unsafe {
        let _ = CString::from_raw(value);
    }
}

#[no_mangle]
pub extern "C" fn llm_ext_last_error_message() -> *mut c_char {
    match take_last_error() {
        Some(message) => string_to_c(&message),
        None => ptr::null_mut(),
    }
}
