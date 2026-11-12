#!/usr/bin/env ruby
# frozen_string_literal: true

# Packages one precompiled platform gem (no extension, no compile at install).
#
#   ruby scripts/build_platform_gem.rb <platform> <path-to-cdylib>
#
# <platform> is a RubyGems platform string (x86_64-linux, x86_64-linux-musl,
# aarch64-linux, arm64-darwin, x86_64-darwin, x64-mingw-ucrt). The cdylib is
# staged into lib/sirena/native/ under its own file name, which is the name
# Sirena::Native looks for. The gem lands in pkg/. Run from bindings/ruby.
require "fileutils"
require "rubygems"

platform, lib_path = ARGV
abort "usage: build_platform_gem.rb <platform> <cdylib>" unless platform && lib_path
abort "no such cdylib: #{lib_path}" unless File.file?(lib_path)

expected = { /darwin/ => "libetassia_ffi.dylib", /mingw|mswin/ => "etassia_ffi.dll", /linux/ => "libetassia_ffi.so" }
want = expected.find { |re, _| platform.match?(re) }&.last or abort "unsupported platform: #{platform}"
abort "#{File.basename(lib_path)} is not #{want} (required for #{platform})" unless File.basename(lib_path) == want

root = File.expand_path("..", __dir__)
staged_dir = File.join(root, "lib", "sirena", "native")
FileUtils.mkdir_p(staged_dir)
staged = File.join(staged_dir, want)
FileUtils.cp(lib_path, staged)
begin
  built = nil
  Dir.chdir(root) do
    ENV["SIRENA_GEM_PLATFORM"] = platform
    abort "gem build failed" unless system("gem", "build", "sirena-native.gemspec")
    built = Dir["sirena-native-*.gem"].max_by { |f| File.mtime(f) }
  end
  FileUtils.mkdir_p(File.join(root, "pkg"))
  FileUtils.mv(File.join(root, built), File.join(root, "pkg", built), force: true)
  puts "pkg/#{built}"
ensure
  FileUtils.rm_f(staged)
end
