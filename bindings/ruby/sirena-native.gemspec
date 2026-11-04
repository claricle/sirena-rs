# frozen_string_literal: true

Gem::Specification.new do |spec|
  spec.name = "sirena-native"
  spec.version = "0.0.0"
  spec.authors = ["Sirena contributors"]
  spec.summary = "Ruby binding to the sirena-ffi C ABI, with a pure-Ruby fallback"
  spec.license = "BSD-2-Clause"
  spec.required_ruby_version = ">= 3.1"
  spec.files = Dir["lib/**/*.rb"]
  spec.require_paths = ["lib"]
  spec.add_dependency "ffi", "~> 1.16"
  spec.metadata["rubygems_mfa_required"] = "true"
end
