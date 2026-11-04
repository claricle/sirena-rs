# sirena-py

Python binding (PyO3 + maturin, abi3) over `sirena-core`.

    pip install maturin pytest
    cd bindings/python && maturin develop --release && pytest

`render(source) -> str` returns the SVG, byte-identical to the Ruby gem for every
ported type. `Unported` means the Rust core has no renderer for that type yet (use
the Ruby path); `SirenaError` (with `.error_class`, `.line`, `.col`) is an invalid
source. `detect(source) -> str` names the diagram type.
