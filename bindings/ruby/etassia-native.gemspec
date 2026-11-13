# frozen_string_literal: true

Gem::Specification.new do |spec|
  spec.name = "sirena-native"
  spec.version = "0.0.0"
  spec.authors = ["Sirena contributors"]
  spec.summary = "Ruby binding to the sirena-ffi C ABI, with a pure-Ruby fallback"
  spec.license = "BSD-2-Clause"
  spec.required_ruby_version = ">= 3.1"
  spec.files = Dir["lib/**/*.rb", "ext/sirena_cdylib/extconf.rb"]
  # Binaries never enter the source gem: the cross-gem build vendors the
  # triple's cdylib into lib/sirena/native/ only when SIRENA_VENDOR_CDYLIB
  # is set (the parsanol-ruby contract). Plain `gem build` stays
  # binary-free and falls back to the pure-Ruby renderer at runtime.
  spec.files.reject! do |f|
    f =~ /\.(dll|so|dylib)\Z/ &&
      !(ENV["SIRENA_VENDOR_CDYLIB"] == "1" &&
        f =~ %r{\Alib/sirena/native/(?:libsirena_ffi|sirena_ffi)\.(?:so|dylib|dll)\Z})
  end
  # The cross-gem matrix drives this per target: its Makefile builds the
  # sirena-ffi cdylib for the triple and vendors it into the gem.
  spec.extensions = ["ext/sirena_cdylib/extconf.rb"]
  spec.require_paths = ["lib"]
  spec.add_dependency "ffi", "~> 1.16"
  spec.metadata["rubygems_mfa_required"] = "true"
end
