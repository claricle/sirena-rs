# frozen_string_literal: true

# sirena-native is a fiddle gem: there is no Ruby C extension to
# compile. This Makefile contract builds the sirena-ffi cdylib for the
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

  all:
  \tcargo build --release -p sirena-ffi$(if $(CROSS_TARGET), --target $(CROSS_TARGET),)
  \tmkdir -p ../../lib/sirena/native
  \tif [ -n "$(CROSS_TARGET)" ]; then \\
  \t\tcp target/$(CROSS_TARGET)/release/libsirena_ffi.* ../../lib/sirena/native/ 2>/dev/null; \\
  \t\tcp target/$(CROSS_TARGET)/release/sirena_ffi.* ../../lib/sirena/native/ 2>/dev/null || true; \\
  \telse \\
  \t\tcp target/release/libsirena_ffi.* ../../lib/sirena/native/ 2>/dev/null; \\
  \tfi
  install:
  clean:
MAKEFILE

File.write("Makefile", makefile)
warn "sirena-native: Makefile contract emits the sirena-ffi cdylib into lib/sirena/native (root: #{sirena_root})"
