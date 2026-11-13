# sirena-ffi — C ABI

`sirena-ffi` builds a `cdylib` (`libsirena_ffi.dylib` / `.so`, `sirena_ffi.dll`)
exposing three functions over `sirena-core`. Build with `cargo build -p sirena-ffi`.

## Functions

```c
typedef struct { uint8_t *ptr; size_t len; int status; } SirenaBuf;

SirenaBuf sirena_render(const char *src, size_t len, const char *opts_json); // opts_json: reserved, may be NULL
SirenaBuf sirena_detect(const char *src, size_t len);
void      sirena_free(SirenaBuf buf);  // release each returned buffer exactly once
```

- `src` is UTF-8 with `len` bytes. It need not be NUL-terminated.
- Each call returns a heap buffer. Read `len` bytes from `ptr`, then call
  `sirena_free` on the same struct. Do not free twice; do not free with `free()`.
- `ptr` holds `len` bytes plus a trailing NUL, so C strings are safe too.

## Status and payload

| `status` | `ptr`/`len` contains |
|---|---|
| `0` | success: SVG text (`sirena_render`) or diagram type name (`sirena_detect`) |
| `1` | error: JSON `{"class", "message", "line", "col"}` |

Error classes seen by callers:

| `class` | Meaning | Caller action |
|---|---|---|
| `Unported` | The Rust core does not yet port this diagram or construct (`message` names it, e.g. `"sankey"`). | Fall back to the Ruby renderer. |
| `DiagramTypeError` | `sirena_detect` found no diagram type. | Report an error. |
| `InvalidArgument` | `src` is NULL. | Report an error. |
| `InvalidEncoding` | `src` is not valid UTF-8. | Report an error. |
| `ParseError` | Source is malformed. `line`/`col` are 1-based. | Report an error. |

`Unported` is the only class that means "try another renderer". Every other
error is final.

## Ruby binding

`bindings/ruby` (gem `sirena-native`) loads this library through `ffi`:

1. `ENV["SIRENA_NATIVE_LIB"]`, an exact file path. When set, nothing else is searched.
2. `target/release/`, then `target/debug/` under the repository root.

`Sirena::Native.render(src)` returns the SVG from the library. It falls back to a
pure-Ruby callable (default: `Sirena.render` from the `sirena` gem) when no library is
found or the call returns `Unported`. Run the spec with
`cd bindings/ruby && ruby -Ilib spec/native_spec.rb`.

## Ownership

The C ABI is a contract. Changing a function signature, the `SirenaBuf` layout, or an
error `class` string breaks the binding. Update this file and the Ruby binding in the
same change.
