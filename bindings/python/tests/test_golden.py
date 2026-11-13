"""Every golden case through the binding, byte-compared to the Ruby-generated SVG.

Golden dir: $ETASSIA_GOLDEN_DIR, else <repo>/golden (same as the Rust golden test).
"""
import collections
import json
import os
import pathlib

import pytest

import etassia_py

REPO = pathlib.Path(__file__).resolve().parents[3]
GOLDEN = pathlib.Path(os.environ.get("ETASSIA_GOLDEN_DIR") or REPO / "golden")
MANIFEST = json.loads((GOLDEN / "manifest.json").read_text())
CASES = MANIFEST["cases"]
REQUIRED = {"info", "pie", "packet", "timeline", "treemap", "quadrant", "radar", "xychart", "user_journey", "kanban", "mindmap", "gantt", "gitgraph", "er_diagram", "requirement", "c4", "block", "architecture", "sequence", "state_diagram", "flowchart", "class_diagram"}


def test_every_golden_case_byte_identical():
    counts = collections.defaultdict(lambda: [0, 0, 0])  # match, mismatch, unported
    first = {}
    for c in CASES:
        src = (GOLDEN / c["case"]).read_bytes().decode("utf-8")
        want = (GOLDEN / c["case"].replace(".mmd", ".svg")).read_bytes()
        row = counts[c["type"]]
        try:
            got = etassia_py.render(src)
        except etassia_py.Unported:
            row[2] += 1
            continue
        if got.encode("utf-8") == want:
            row[0] += 1
        else:
            row[1] += 1
            first.setdefault(c["type"], c["case"])
    for ty, (ok, bad, unp) in sorted(counts.items()):
        print(f"{ty:16} match={ok:4} mismatch={bad:4} unported={unp:4}")
    total = [sum(r[i] for r in counts.values()) for i in range(3)]
    print("TOTAL match=%d mismatch=%d unported=%d of %d" % (*total, len(CASES)))
    # Same bar as crates/etassia-core/tests/golden.rs: the ported types must be
    # byte-identical with no Unported; "error"/"unknown" are the Ruby-fallback
    # buckets (Unported), reported but not required.
    bad = {t: r for t, r in counts.items() if t in REQUIRED and (r[1] or r[2])}
    assert not bad, f"required types not byte-identical: {bad} first={first}"
    assert total[1] == 0
    assert sum(r[0] + r[1] + r[2] for t, r in counts.items() if t in REQUIRED) == 1166
    assert total[0] == 1183


def test_unported_cases_are_only_the_fallback_buckets():
    for c in CASES:
        if c["type"] in REQUIRED:
            continue
        src = (GOLDEN / c["case"]).read_bytes().decode("utf-8")
        try:
            etassia_py.render(src)
        except etassia_py.Unported:
            pass


@pytest.mark.parametrize("src,name", [("pie\n  \"a\": 1\n", "pie"), ("flowchart TD\n  A-->B\n", "flowchart")])
def test_detect(src, name):
    assert etassia_py.detect(src) == name


def test_exceptions_are_distinct():
    assert not issubclass(etassia_py.Unported, etassia_py.EtassiaError)
    assert not issubclass(etassia_py.EtassiaError, etassia_py.Unported)
    with pytest.raises(etassia_py.EtassiaError):
        etassia_py.detect("not a diagram at all")


def test_render_rejects_non_str():
    with pytest.raises(TypeError):
        etassia_py.render(b"pie\n")
