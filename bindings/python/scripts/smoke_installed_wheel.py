"""Smoke an INSTALLED sirena-py wheel: render one golden case, byte-compare.

usage: python smoke_installed_wheel.py <golden.mmd> <golden.svg>
Run from outside the source tree so the installed wheel is what gets imported.
"""
import pathlib
import sys

import sirena_py

mmd, svg = (pathlib.Path(p) for p in sys.argv[1:3])
if "bindings" in pathlib.Path(sirena_py.__file__).parts and "site-packages" not in sirena_py.__file__:
    sys.exit(f"sirena_py imported from the source tree: {sirena_py.__file__}")
got = sirena_py.render(mmd.read_bytes().decode("utf-8")).encode("utf-8")
want = svg.read_bytes()
if got != want:
    sys.exit(f"MISMATCH: rendered {len(got)} bytes, golden {len(want)} bytes")
print(f"OK {sirena_py.__file__}: {mmd.name} byte-identical ({len(got)} bytes)")
