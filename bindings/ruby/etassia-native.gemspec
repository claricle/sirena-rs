# frozen_string_literal: true

require_relative "lib/etassia/native/version"

Gem::Specification.new do |spec|
  spec.name = "etassia-native"
  spec.version = Etassia::Native::VERSION
  spec.authors = ["Etassia contributors"]
  spec.summary = "Ruby binding to the etassia-ffi C ABI, with a pure-Ruby fallback"
  spec.license = "BSD-2-Clause"
  spec.required_ruby_version = ">= 3.1"
  spec.files = Dir["lib/**/*.rb"]
  # Binaries are never tracked in git, and the ruby (source) gem stays
  # slim: the etassia-ffi cdylib is vendored ONLY into platform gems by
  # the cross-gem build (rakelib/compile.rake's native:<platform>
  # tasks), never by a plain `gem build`.
  spec.files.reject! do |f|
    f =~ /\.(dll|so|dylib)\Z/ &&
      !(ENV["ETASSIA_VENDOR_CDYLIB"] == "1" &&
        f =~ %r{\Alib/etassia/native/(?:libetassia_ffi|etassia_ffi)\.(?:so|dylib|dll)\Z})
  end
  spec.require_paths = ["lib"]
  spec.add_dependency "ffi", "~> 1.16"
  spec.metadata["rubygems_mfa_required"] = "true"
end
