//! Import-free wasm32 surface for the TypeScript binding (`bindings/ts`).
//!
//! Plain C-ABI exports over linear memory, no wasm-bindgen: the host copies the
//! UTF-8 source in with `sirena_alloc`, calls `sirena_render` / `sirena_detect`,
//! then reads the result through `sirena_result_ptr` / `sirena_result_len`.
//! Rendering happens entirely here, so number formatting is the Rust formatter's.
//! Status 0 = SVG / type name; 1 = JSON `{class,message,line,col}` (class
//! "Unported" means the caller should use the Ruby fallback).
use sirena_core::Error;
use std::cell::RefCell;

thread_local! {
    static RESULT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

fn store(r: Result<String, Error>) -> i32 {
    let (bytes, status) = match r {
        Ok(s) => (s.into_bytes(), 0),
        Err(e) => (e.to_json().into_bytes(), 1),
    };
    RESULT.with(|b| *b.borrow_mut() = bytes);
    status
}

fn parse_err(class: &str, message: String) -> Error {
    Error::Parse { class: class.into(), message, line: 0, col: 0 }
}

unsafe fn src<'a>(ptr: *const u8, len: usize) -> Result<&'a str, Error> {
    if ptr.is_null() {
        return Err(parse_err("InvalidArgument", "null source".into()));
    }
    std::str::from_utf8(std::slice::from_raw_parts(ptr, len))
        .map_err(|e| parse_err("InvalidEncoding", e.to_string()))
}

/// Allocate `len` bytes of linear memory for the host to fill.
#[no_mangle]
pub extern "C" fn sirena_alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len.max(1));
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// Release a buffer from `sirena_alloc`.
///
/// # Safety
/// `ptr`/`len` must be exactly a prior `sirena_alloc(len)`, freed once.
#[no_mangle]
pub unsafe extern "C" fn sirena_dealloc(ptr: *mut u8, len: usize) {
    drop(Vec::from_raw_parts(ptr, 0, len.max(1)));
}

/// Render the UTF-8 source at `ptr`/`len`. Returns the status.
///
/// # Safety
/// `ptr` must point to `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn sirena_render(ptr: *const u8, len: usize) -> i32 {
    store(src(ptr, len).and_then(sirena_core::render))
}

/// Detect the diagram type. Returns the status.
///
/// # Safety
/// `ptr` must point to `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn sirena_detect(ptr: *const u8, len: usize) -> i32 {
    store(src(ptr, len).and_then(|s| {
        sirena_core::detect(s)
            .ok_or_else(|| parse_err("DiagramTypeError", "cannot detect diagram type".into()))
    }))
}

#[no_mangle]
pub extern "C" fn sirena_result_ptr() -> *const u8 {
    RESULT.with(|b| b.borrow().as_ptr())
}

#[no_mangle]
pub extern "C" fn sirena_result_len() -> usize {
    RESULT.with(|b| b.borrow().len())
}

