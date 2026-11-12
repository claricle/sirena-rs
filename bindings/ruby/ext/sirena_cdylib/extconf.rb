# frozen_string_literal: true

# sirena-native is a fiddle gem: there is no Ruby C extension to
# compile. This Makefile contract builds the etassia-ffi cdylib for the
# active (cross) target and vendors it into lib/sirena/native/, where
# the platform gem's loader (Sirena::Native::PACKAGED_DIR) finds it.
# On engines with no C-toolchain contract (JRuby, TruffleRuby), emit a
# no-op Makefile so the gem installs cleanly - lib/sirena/native.rb
# rescues LoadError and falls back to the pure-Ruby renderer.

if RUBY_ENGINE != "ruby"
  File.write("Makefile", <<~MAKEFILE)
    all:
    install:
    clean:
  MAKEFILE
  warn "sirena-native: skipping the cdylib build on #{RUBY_ENGINE} (pure-Ruby fallback)"
  return
end

sirena_root = File.expand_path("../..", __dir__)

makefile = <<~MAKEFILE
  CROSS_TARGET := $(CARGO_BUILD_TARGET)$(RUST_TARGET)

  # Windows links the CRT statically so the DLL needs no VC redistributable
  # (the platform-gems matrix set this via RUSTFLAGS).
  ifneq (,$(findstring windows,$(CROSS_TARGET)))
    export RUSTFLAGS := -C target-feature=+crt-static
  endif

  all:
  \tcargo build --release -p etassia-ffi$(if $(CROSS_TARGET), --target $(CROSS_TARGET),)
  \tmkdir -p ../../lib/sirena/native
  \tif [ -n "$(CROSS_TARGET)" ]; then \\
  \t\tcp target/$(CROSS_TARGET)/release/libetassia_ffi.* ../../lib/sirena/native/ 2>/dev/null; \\
  \t\tcp target/$(CROSS_TARGET)/release/etassia_ffi.* ../../lib/sirena/native/ 2>/dev/null || true; \\
  \telse \\
  \t\tcp target/release/libetassia_ffi.* ../../lib/sirena/native/ 2>/dev/null; \\
  \tfi
  install:
  clean:
MAKEFILE

File.write("Makefile", makefile)
warn "sirena-native: Makefile contract emits the etassia-ffi cdylib into lib/sirena/native (root: #{sirena_root})"
