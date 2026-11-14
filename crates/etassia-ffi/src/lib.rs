//! C ABI: etassia_render / etassia_detect / etassia_free.
//!
//! Every call returns a heap buffer the caller releases with `etassia_free`.
//! `status` (out) is 0 on success (buffer = SVG / type name), 1 on error
//! (buffer = JSON `{class,message,line,col}`; class "Unported" means use the
//! Ruby fallback). `out_len` receives the byte length (buffer is also NUL-terminated).
use etassia::Error;
use std::os::raw::{c_char, c_int};

#[repr(C)]
pub struct EtassiaBuf {
    pub ptr: *mut u8,
    pub len: usize,
    pub status: c_int,
}

fn into_buf(r: Result<String, Error>) -> EtassiaBuf {
    let (mut bytes, status) = match r {
        Ok(s) => (s.into_bytes(), 0),
        Err(e) => (e.to_json().into_bytes(), 1),
    };
    let len = bytes.len();
    bytes.push(0);
    let boxed = bytes.into_boxed_slice();
    EtassiaBuf { ptr: Box::into_raw(boxed) as *mut u8, len, status }
}

unsafe fn src<'a>(ptr: *const c_char, len: usize) -> Result<&'a str, Error> {
    if ptr.is_null() {
        return Err(Error::Parse { class: "InvalidArgument".into(), message: "null source".into(), line: 0, col: 0 });
    }
    std::str::from_utf8(std::slice::from_raw_parts(ptr as *const u8, len)).map_err(|e| Error::Parse {
        class: "InvalidEncoding".into(),
        message: e.to_string(),
        line: 0,
        col: 0,
    })
}

/// Render `src` (UTF-8, `len` bytes). `opts_json` is reserved (may be NULL).
///
/// # Safety
/// `src_ptr` must point to `len` readable bytes; `opts_json` NULL or NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn etassia_render(src_ptr: *const c_char, len: usize, _opts_json: *const c_char) -> EtassiaBuf {
    into_buf(src(src_ptr, len).and_then(etassia::render))
}

/// Detect the diagram type name of `src`.
///
/// # Safety
/// `src_ptr` must point to `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn etassia_detect(src_ptr: *const c_char, len: usize) -> EtassiaBuf {
    into_buf(src(src_ptr, len).and_then(|s| {
        etassia::detect(s).ok_or_else(|| Error::Parse {
            class: "DiagramTypeError".into(),
            message: "cannot detect diagram type".into(),
            line: 0,
            col: 0,
        })
    }))
}

/// Release a buffer returned by this library.
///
/// # Safety
/// `buf` must come from `etassia_render`/`etassia_detect`, freed at most once.
#[no_mangle]
pub unsafe extern "C" fn etassia_free(buf: EtassiaBuf) {
    if !buf.ptr.is_null() {
        drop(Box::from_raw(std::slice::from_raw_parts_mut(buf.ptr, buf.len + 1)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_unported_then_free() {
        let s = "pie\n  \"a\": 1\n";
        let buf = unsafe { etassia_render(s.as_ptr() as *const c_char, s.len(), std::ptr::null()) };
        let body = unsafe { std::str::from_utf8(std::slice::from_raw_parts(buf.ptr, buf.len)).unwrap().to_string() };
        // Either SVG (status 0) or a JSON error (status 1); both must be well-formed.
        if buf.status == 0 {
            assert!(body.starts_with("<svg"));
        } else {
            let v: serde_json::Value = serde_json::from_str(&body).unwrap();
            assert!(v["class"].is_string());
        }
        unsafe { etassia_free(buf) };
    }

    #[test]
    fn null_source_is_error() {
        let buf = unsafe { etassia_render(std::ptr::null(), 0, std::ptr::null()) };
        assert_eq!(buf.status, 1);
        unsafe { etassia_free(buf) };
    }
}
