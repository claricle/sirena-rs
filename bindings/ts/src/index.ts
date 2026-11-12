import { readFileSync } from "node:fs";

/** Thrown when the Rust runtime has no renderer for this diagram type or construct; use the Ruby fallback. */
export class UnportedError extends Error {
  override name = "UnportedError";
}

/** Thrown when the source is invalid (parse error, undetectable type, bad encoding). */
export class EtassiaError extends Error {
  constructor(
    message: string,
    readonly errorClass: string,
    readonly line: number,
    readonly col: number,
  ) {
    super(message);
    this.name = "EtassiaError";
  }
}

interface Exports {
  memory: WebAssembly.Memory;
  sirena_alloc(len: number): number;
  sirena_dealloc(ptr: number, len: number): void;
  sirena_render(ptr: number, len: number): number;
  sirena_detect(ptr: number, len: number): number;
  sirena_result_ptr(): number;
  sirena_result_len(): number;
}

// The wasm keeps wasm-bindgen import stubs from parsanol's getrandom dependency
// (hash seeding). In this unprocessed module getRandomValues receives a plain
// (ptr, len) pair into linear memory, so it is filled here; every other stub is
// unreachable on the render path and throws if called.
let memory: WebAssembly.Memory | undefined;
const imports = new Proxy({}, {
  get: (_t, mod: string) =>
    new Proxy({}, {
      get: (_u, name: string) => {
        if (name.startsWith("__wbg_getRandomValues_")) {
          return (ptr: number, len: number) => {
            globalThis.crypto.getRandomValues(new Uint8Array(memory!.buffer, ptr, len));
          };
        }
        return () => {
          throw new Error(`unexpected wasm import ${mod}.${name}`);
        };
      },
    }),
}) as WebAssembly.Imports;

let instance: Exports | undefined;

function wasm(): Exports {
  if (!instance) {
    instance = new WebAssembly.Instance(
      new WebAssembly.Module(readFileSync(new URL("./etassia.wasm", import.meta.url))),
      imports,
    ).exports as unknown as Exports;
    memory = instance.memory;
  }
  return instance;
}

function call(fn: "sirena_render" | "sirena_detect", source: string): string {
  const w = wasm();
  const bytes = new TextEncoder().encode(source);
  const ptr = w.sirena_alloc(bytes.length);
  new Uint8Array(w.memory.buffer, ptr, bytes.length).set(bytes);
  let status: number;
  try {
    status = w[fn](ptr, bytes.length);
  } finally {
    w.sirena_dealloc(ptr, bytes.length);
  }
  const out = new TextDecoder().decode(
    new Uint8Array(w.memory.buffer, w.sirena_result_ptr(), w.sirena_result_len()),
  );
  if (status === 0) return out;
  const e = JSON.parse(out) as { class: string; message: string; line: number; col: number };
  if (e.class === "Unported") throw new UnportedError(e.message);
  throw new EtassiaError(e.message, e.class, e.line, e.col);
}

/**
 * Render a Mermaid diagram to SVG. The SVG is produced entirely by the Rust
 * runtime (no JS-side reformatting). Throws UnportedError when the Ruby
 * fallback must be used, EtassiaError for invalid input.
 */
export function render(source: string): string {
  return call("sirena_render", source);
}

/** Detect the diagram type name of `source`. Throws EtassiaError when undetectable. */
export function detect(source: string): string {
  return call("sirena_detect", source);
}
