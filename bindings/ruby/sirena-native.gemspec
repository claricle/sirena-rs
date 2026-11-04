# frozen_string_literal: true

Gem::Specification.new do |spec|
  spec.name = "sirena-native"
  spec.version = "0.0.0"
  spec.authors = ["Sirena contributors"]
  spec.summary = "Ruby binding to the sirena-ffi C ABI, with a pure-Ruby fallback"
  spec.license = "BSD-2-Clause"
  spec.required_ruby_version = ">= 3.1"
  spec.files = Dir["lib/**/*.rb"]
  # Platform gems (built by scripts/build_platform_gem.rb) carry the precompiled
  # cdylib and are tagged with the target platform. Without SIRENA_GEM_PLATFORM
  # this is the pure-Ruby gem, which falls back when no native library is found.
  if (platform = ENV.fetch("SIRENA_GEM_PLATFORM", nil)) && !platform.empty?
    spec.platform = Gem::Platform.new(platform)
    spec.files += Dir["lib/sirena/native/*.{so,dylib,dll}"]
  end
  spec.require_paths = ["lib"]
  spec.add_dependency "ffi", "~> 1.16"
  spec.metadata["rubygems_mfa_required"] = "true"
end
