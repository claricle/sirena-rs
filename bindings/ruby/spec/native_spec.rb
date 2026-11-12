# frozen_string_literal: true

# Run: ruby -Ilib spec/native_spec.rb   (from bindings/ruby)
require "minitest/autorun"
require "tmpdir"
require "sirena/native"

module NativeSpecSupport
  REPO_ROOT = File.expand_path("../../..", __dir__)
  PIE_SOURCE = File.join(REPO_ROOT, "golden", "pie", "pie_spec-0b1f3197.mmd")
  PIE_SVG = File.join(REPO_ROOT, "golden", "pie", "pie_spec-0b1f3197.svg")
  UNPORTED_SOURCE = "sankey-beta\na,b,1\n"
  MISSING_LIB = "/nonexistent/sirena-native-spec/libetassia_ffi.dylib"

  def self.cargo
    return ENV["CARGO"] if ENV["CARGO"] && !ENV["CARGO"].empty?

    candidate = File.join(Dir.home, ".cargo", "bin", "cargo")
    File.executable?(candidate) ? candidate : "cargo"
  end

  # Builds the cdylib once. Returns nil on success, or the skip reason when cargo fails.
  def self.build_reason
    return @build_reason if defined?(@build_reason)

    log = File.join(Dir.tmpdir, "etassia-ffi-native-spec-build.log")
    ok = system(cargo, "build", "-p", "etassia-ffi", chdir: REPO_ROOT, out: log, err: log)
    @build_reason = ok ? nil : "cargo build -p etassia-ffi failed (see #{log})"
  end

  def self.debug_lib
    ext = RbConfig::CONFIG["host_os"].match?(/darwin/) ? "dylib" : "so"
    File.join(REPO_ROOT, "target", "debug", "libetassia_ffi.#{ext}")
  end
end

class NativeLoadedLibTest < Minitest::Test
  def setup
    reason = NativeSpecSupport.build_reason
    skip "loaded-lib path skipped: #{reason}" if reason
    @saved_env = ENV.fetch("SIRENA_NATIVE_LIB", nil)
    ENV["SIRENA_NATIVE_LIB"] = NativeSpecSupport.debug_lib
  end

  def teardown
    ENV["SIRENA_NATIVE_LIB"] = @saved_env
  end

  def test_render_uses_native_library_and_returns_svg
    no_fallback = ->(_src) { flunk "native library present; fallback must not run" }
    svg = Sirena::Native.render(File.read(NativeSpecSupport::PIE_SOURCE), fallback: no_fallback)

    assert_equal File.read(NativeSpecSupport::PIE_SVG), svg
  end

  def test_detect_reports_diagram_type
    assert_equal "pie", Sirena::Native.detect(File.read(NativeSpecSupport::PIE_SOURCE))
  end

  def test_non_unported_error_raises_render_error_without_fallback
    error = assert_raises(Sirena::Native::RenderError) do
      Sirena::Native.detect("")
    end
    assert_equal "DiagramTypeError", error.error_class
  end

  def test_native_reports_unported_for_declined_type
    error = assert_raises(Sirena::Native::Unported) { Sirena::Native.library.render(NativeSpecSupport::UNPORTED_SOURCE) }
    assert_equal "sankey", error.message
  end

  def test_unported_from_native_falls_back_with_same_source
    seen = nil
    fallback = lambda do |src|
      seen = src
      "<svg>fallback</svg>"
    end

    assert_equal "<svg>fallback</svg>", Sirena::Native.render(NativeSpecSupport::UNPORTED_SOURCE, fallback: fallback)
    assert_equal NativeSpecSupport::UNPORTED_SOURCE, seen
  end
end

class NativeMissingLibTest < Minitest::Test
  def setup
    @saved_env = ENV.fetch("SIRENA_NATIVE_LIB", nil)
    ENV["SIRENA_NATIVE_LIB"] = NativeSpecSupport::MISSING_LIB
  end

  def teardown
    ENV["SIRENA_NATIVE_LIB"] = @saved_env
  end

  def test_library_path_is_nil_when_override_points_nowhere
    refute Sirena::Native.available?
    assert_nil Sirena::Native.library_path
  end

  def test_render_falls_back_when_library_is_missing
    fallback = ->(src) { "fallback:#{src}" }

    assert_equal "fallback:pie", Sirena::Native.render("pie", fallback: fallback)
  end

  def test_detect_without_library_raises_unavailable
    assert_raises(Sirena::Native::Unavailable) { Sirena::Native.detect("pie") }
  end

  def test_default_fallback_reports_missing_pure_ruby_gem
    # The 'sirena' gem is not installed in this spec environment.
    begin
      require "sirena"
      skip "the sirena gem is installed here"
    rescue LoadError
      # expected: the pure-Ruby gem is absent
    end

    assert_raises(Sirena::Native::Unavailable) { Sirena::Native.render("pie") }
  end
end
