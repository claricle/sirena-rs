#!/usr/bin/env ruby
# frozen_string_literal: true

# Smoke for an INSTALLED etassia-native gem (never the source tree).
#
#   ruby scripts/smoke_installed_gem.rb native|fallback <golden.mmd> <golden.svg>
#
# native:   the packaged cdylib loads from the gem dir and renders the golden
#           byte-for-byte; the pure-Ruby fallback must not run.
# fallback: the pure (ruby-platform) gem has no library and calls the fallback.
# Run it from outside the repository with GEM_HOME/GEM_PATH pointing at the install.
require "etassia/native"

mode, source_path, golden_path = ARGV
abort "usage: smoke_installed_gem.rb native|fallback <mmd> <svg>" unless %w[native
                                                                            fallback].include?(mode) && golden_path

source = File.binread(source_path)
golden = File.binread(golden_path)
gem_root = Gem.loaded_specs.fetch("etassia-native").full_gem_path

case mode
when "native"
  path = Etassia::Native.library_path or abort "FAIL: no native library found in installed gem"
  unless File.expand_path(path).start_with?(File.expand_path(gem_root))
    abort "FAIL: library #{path} is not inside #{gem_root}"
  end
  svg = Etassia::Native.render(source, fallback: ->(_) { abort "FAIL: fell back to pure Ruby" })
  abort "FAIL: rendered SVG differs from golden" unless svg.b == golden
  abort "FAIL: detect returned #{Etassia::Native.detect(source).inspect}" unless Etassia::Native.detect(source) == "pie"
  puts "ok native #{Gem.loaded_specs["etassia-native"].platform} #{File.basename(path)} #{svg.bytesize} bytes"
when "fallback"
  abort "FAIL: pure gem unexpectedly has a library at #{Etassia::Native.library_path}" if Etassia::Native.available?
  svg = Etassia::Native.render(source, fallback: ->(_) { "fallback-called" })
  abort "FAIL: fallback not used" unless svg == "fallback-called"
  puts "ok fallback #{Gem.loaded_specs["etassia-native"].platform}"
end
