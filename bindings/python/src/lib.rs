//! PyO3 bindings over `etassia-rs`: `render`, `detect`, and two exceptions.
use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use etassia_rs::Error;

create_exception!(_native, EtassiaError, PyException, "Source is invalid for its diagram type.");
create_exception!(
    _native,
    Unported,
    PyException,
    "The Rust core has no renderer for this diagram type yet; use the Ruby path."
);

fn to_py(py: Python<'_>, e: Error) -> PyErr {
    match e {
        Error::Unported(t) => Unported::new_err(t),
        Error::Parse { class, message, line, col } => {
            let err = EtassiaError::new_err(format!("{class} at {line}:{col}: {message}"));
            let v = err.value(py);
            // Attributes are best-effort extras; the message already carries them.
            let _ = v.setattr("error_class", class);
            let _ = v.setattr("line", line);
            let _ = v.setattr("col", col);
            err
        }
    }
}

/// Render Mermaid `source` to an SVG string.
#[pyfunction]
fn render(py: Python<'_>, source: &str) -> PyResult<String> {
    py.detach(|| etassia_rs::render(source)).map_err(|e| to_py(py, e))
}

/// Detect the diagram type name of `source`, or raise `EtassiaError`.
#[pyfunction]
fn detect(source: &str) -> PyResult<String> {
    etassia_rs::detect(source).map(|s| s.to_string()).ok_or_else(|| {
        EtassiaError::new_err("DiagramTypeError at 0:0: cannot detect diagram type")
    })
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(render, m)?)?;
    m.add_function(wrap_pyfunction!(detect, m)?)?;
    m.add("EtassiaError", m.py().get_type::<EtassiaError>())?;
    m.add("Unported", m.py().get_type::<Unported>())?;
    Ok(())
}
